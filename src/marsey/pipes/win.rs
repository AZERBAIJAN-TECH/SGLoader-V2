use std::ffi::OsStr;
use std::iter;
use std::os::windows::ffi::OsStrExt;

use windows::Win32::Foundation::{
    CloseHandle, ERROR_IO_PENDING, ERROR_PIPE_CONNECTED, GetLastError, HANDLE, WAIT_OBJECT_0,
};
use windows::Win32::Storage::FileSystem::{FILE_FLAGS_AND_ATTRIBUTES, FlushFileBuffers, WriteFile};
use windows::Win32::System::IO::{GetOverlappedResult, OVERLAPPED};
use windows::Win32::System::Pipes::{
    ConnectNamedPipe, CreateNamedPipeW, DisconnectNamedPipe, NAMED_PIPE_MODE,
};
use windows::Win32::System::Threading::{CreateEventW, WaitForSingleObject};
use windows::core::PCWSTR;

const PIPE_ACCESS_OUTBOUND: u32 = 0x00000002;
const PIPE_TYPE_BYTE: u32 = 0x00000000;
const PIPE_READMODE_BYTE: u32 = 0x00000000;
const PIPE_WAIT: u32 = 0x00000000;
const PIPE_UNLIMITED_INSTANCES: u32 = 255;
const FILE_FLAG_OVERLAPPED: u32 = 0x40000000;

pub fn send_named_pipe_utf8(pipe_name: &str, data: &str, timeout_ms: u32) -> Result<(), String> {
    let full_name = format!("\\\\.\\pipe\\{pipe_name}");
    let name_w = to_wide_null(&full_name);

    unsafe {
        let open_mode = FILE_FLAGS_AND_ATTRIBUTES(PIPE_ACCESS_OUTBOUND | FILE_FLAG_OVERLAPPED);
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

        // Overlapped connect with timeout.
        let event = CreateEventW(None, true, false, None)
            .map_err(|e| format!("CreateEventW failed: {e}"))?;
        let event_guard = HandleGuard(event);

        let mut overlapped = OVERLAPPED {
            hEvent: event_guard.0,
            ..Default::default()
        };

        let res = ConnectNamedPipe(handle, Some(&mut overlapped));
        if res.is_err() {
            let err = GetLastError();
            if err == ERROR_PIPE_CONNECTED {
                // Connected between CreateNamedPipe and ConnectNamedPipe.
            } else if err == ERROR_IO_PENDING {
                let wait = WaitForSingleObject(event_guard.0, timeout_ms);
                if wait != WAIT_OBJECT_0 {
                    let _ = DisconnectNamedPipe(handle);
                    return Err(format!("ConnectNamedPipe timeout after {timeout_ms}ms"));
                }

                let mut transferred: u32 = 0;
                if GetOverlappedResult(handle, &overlapped, &mut transferred, false).is_err() {
                    let _ = DisconnectNamedPipe(handle);
                    return Err(format!("GetOverlappedResult failed: {:?}", GetLastError()));
                }
            } else {
                let _ = DisconnectNamedPipe(handle);
                return Err(format!("ConnectNamedPipe failed: {:?}", err));
            }
        }

        let bytes = data.as_bytes();
        if !bytes.is_empty() {
            let mut written: u32 = 0;
            if WriteFile(handle, Some(bytes), Some(&mut written), None).is_err() {
                let _ = DisconnectNamedPipe(handle);
                return Err(format!("WriteFile failed: {:?}", GetLastError()));
            }
        }

        let _ = FlushFileBuffers(handle);
        let _ = DisconnectNamedPipe(handle);

        Ok(())
    }
}

fn to_wide_null(s: &str) -> Vec<u16> {
    OsStr::new(s).encode_wide().chain(iter::once(0)).collect()
}

struct HandleGuard(HANDLE);

impl Drop for HandleGuard {
    fn drop(&mut self) {
        unsafe {
            let _ = CloseHandle(self.0);
        }
    }
}
