#[cfg(target_os = "windows")]
#[path = "pipes/win.rs"]
mod win;

#[cfg(target_os = "windows")]
pub use win::send_named_pipe_utf8;

#[cfg(not(target_os = "windows"))]
pub fn send_named_pipe_utf8(_pipe_name: &str, _data: &str, _timeout_ms: u32) -> Result<(), String> {
    Err("Marsey IPC поддерживается только на Windows".to_string())
}
