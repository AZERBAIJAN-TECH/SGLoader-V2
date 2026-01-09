use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex, OnceLock,
};

#[cfg(target_os = "windows")]
use windows::Win32::Foundation::{CloseHandle, ERROR_PIPE_CONNECTED, GetLastError, HANDLE};
#[cfg(target_os = "windows")]
use windows::Win32::Storage::FileSystem::{ReadFile};
#[cfg(target_os = "windows")]
use windows::Win32::System::Pipes::{ConnectNamedPipe, CreateNamedPipeW, DisconnectNamedPipe, NAMED_PIPE_MODE};
#[cfg(target_os = "windows")]
use windows::core::PCWSTR;

#[cfg(target_os = "windows")]
use std::ffi::OsStr;
#[cfg(target_os = "windows")]
use std::iter;
#[cfg(target_os = "windows")]
use std::os::windows::ffi::OsStrExt;

const REDIAL_PIPE_PREFIX: &str = "SGLOADER_REDIAL_";

pub struct RedialPipeServer {
    pub pipe_name: String,
    stop: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}

static GLOBAL_SERVER: OnceLock<Mutex<Option<RedialPipeServer>>> = OnceLock::new();

pub fn ensure_global_redial_pipe(
    disable_redial: bool,
    launcher_path: &Path,
) -> Result<Option<String>, String> {
    let m = GLOBAL_SERVER.get_or_init(|| Mutex::new(None));
    let mut guard = m.lock().map_err(|_| "redial mutex poisoned".to_string())?;

    if disable_redial {
        *guard = None;
        return Ok(None);
    }

    if let Some(srv) = guard.as_ref() {
        return Ok(Some(srv.pipe_name.clone()));
    }

    let srv = RedialPipeServer::start_if_enabled(false, launcher_path)?
        .ok_or_else(|| "не удалось запустить redial server".to_string())?;
    let name = srv.pipe_name.clone();
    *guard = Some(srv);
    Ok(Some(name))
}

impl RedialPipeServer {
    pub fn start_if_enabled(disable_redial: bool, launcher_path: &Path) -> Result<Option<Self>, String> {
        if disable_redial {
            return Ok(None);
        }

        let launcher_path = launcher_path.to_path_buf();
        let pipe_name = format!("{REDIAL_PIPE_PREFIX}{}", uuid::Uuid::new_v4());
        let stop = Arc::new(AtomicBool::new(false));
        let stop_thread = stop.clone();
        let pipe_name_thread = pipe_name.clone();

        let thread = std::thread::spawn(move || {
            run_server_loop(&pipe_name_thread, &launcher_path, stop_thread);
        });

        Ok(Some(Self {
            pipe_name,
            stop,
            thread: Some(thread),
        }))
    }
}

impl Drop for RedialPipeServer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);

        // Best-effort: unblock ConnectNamedPipe by connecting once.
        #[cfg(target_os = "windows")]
        {
            let _ = std::fs::OpenOptions::new()
                .write(true)
                .open(format!("\\\\.\\pipe\\{}", self.pipe_name));
        }

        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

fn run_server_loop(pipe_name: &str, launcher_path: &PathBuf, stop: Arc<AtomicBool>) {
    while !stop.load(Ordering::Relaxed) {
        #[cfg(target_os = "windows")]
        {
            if let Ok(Some((reason, connect))) = accept_one(pipe_name) {
                let _ = spawn_launcher_redial(launcher_path, &reason, &connect);
            }
        }

        #[cfg(not(target_os = "windows"))]
        {
            let _ = pipe_name;
            let _ = launcher_path;
            let _ = &stop;
            return;
        }
    }
}

fn spawn_launcher_redial(launcher_path: &Path, reason_cmd: &str, connect_cmd: &str) -> Result<(), String> {
    if reason_cmd.trim().is_empty() || connect_cmd.trim().is_empty() {
        return Ok(());
    }

    let mut cmd = Command::new(launcher_path);
    cmd.arg("--commands")
        .arg(":RedialWait")
        .arg(reason_cmd)
        .arg(connect_cmd);

    cmd.env_remove("DOTNET_TieredPGO");
    cmd.env_remove("DOTNET_TC_QuickJitForLoops");
    cmd.env_remove("DOTNET_ReadyToRun");

    cmd.spawn()
        .map_err(|e| format!("не удалось запустить launcher для redial: {e}"))?;

    Ok(())
}

#[cfg(target_os = "windows")]
fn accept_one(pipe_name: &str) -> Result<Option<(String, String)>, String> {
    unsafe {
        let full_name = format!("\\\\.\\pipe\\{pipe_name}");
        let name_w = to_wide_null(&full_name);

        const PIPE_ACCESS_INBOUND: u32 = 0x00000001;
        const PIPE_TYPE_BYTE: u32 = 0x00000000;
        const PIPE_READMODE_BYTE: u32 = 0x00000000;
        const PIPE_WAIT: u32 = 0x00000000;
        const PIPE_UNLIMITED_INSTANCES: u32 = 255;

        let open_mode = windows::Win32::Storage::FileSystem::FILE_FLAGS_AND_ATTRIBUTES(PIPE_ACCESS_INBOUND);
        let pipe_mode = NAMED_PIPE_MODE(PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT);

        let handle = CreateNamedPipeW(
            PCWSTR(name_w.as_ptr()),
            open_mode,
            pipe_mode,
            PIPE_UNLIMITED_INSTANCES,
            64 * 1024,
            64 * 1024,
            0,
            None,
        );

        if handle == HANDLE::default() || handle.is_invalid() {
            return Err(format!("CreateNamedPipeW failed: {:?}", GetLastError()));
        }

        let _guard = HandleGuard(handle);

        let res = ConnectNamedPipe(handle, None);
        if res.is_err() {
            let err = GetLastError();
            if err != ERROR_PIPE_CONNECTED {
                let _ = DisconnectNamedPipe(handle);
                return Err(format!("ConnectNamedPipe failed: {:?}", err));
            }
        }

        let mut buf = vec![0u8; 8 * 1024];
        let mut read: u32 = 0;
        let ok = ReadFile(handle, Some(buf.as_mut_slice()), Some(&mut read), None);
        let _ = DisconnectNamedPipe(handle);

        if ok.is_err() {
            return Ok(None);
        }

        buf.truncate(read as usize);
        let text = String::from_utf8_lossy(&buf);
        let mut lines = text.lines().map(|l| l.trim()).filter(|l| !l.is_empty());
        let reason = lines.next().unwrap_or("").to_string();
        let connect = lines.next().unwrap_or("").to_string();

        if !reason.starts_with('R') || !connect.starts_with('C') {
            return Ok(None);
        }

        Ok(Some((reason, connect)))
    }
}

#[cfg(target_os = "windows")]
fn to_wide_null(s: &str) -> Vec<u16> {
    OsStr::new(s).encode_wide().chain(iter::once(0)).collect()
}

#[cfg(target_os = "windows")]
struct HandleGuard(HANDLE);

#[cfg(target_os = "windows")]
impl Drop for HandleGuard {
    fn drop(&mut self) {
        unsafe {
            let _ = CloseHandle(self.0);
        }
    }
}
