use std::path::{Path, PathBuf};

pub const APP_DIR_NAME: &str = "SGLoader-v2";

#[cfg(target_os = "windows")]
pub fn data_dir() -> Result<PathBuf, String> {
    let appdata =
        std::env::var("APPDATA").map_err(|_| "APPDATA не найден (Windows)".to_string())?;
    Ok(Path::new(&appdata).join(APP_DIR_NAME))
}

pub fn open_in_file_manager(path: &Path) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("explorer")
            .arg(path)
            .spawn()
            .map(|_| ())
            .map_err(|e| format!("explorer {:?}: {e}", path))
    }

    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .arg(path)
            .spawn()
            .map(|_| ())
            .map_err(|e| format!("open {:?}: {e}", path))
    }

    #[cfg(all(not(target_os = "windows"), not(target_os = "macos")))]
    {
        std::process::Command::new("xdg-open")
            .arg(path)
            .spawn()
            .map(|_| ())
            .map_err(|e| format!("xdg-open {:?}: {e}", path))
    }
}

#[cfg(not(target_os = "windows"))]
pub fn data_dir() -> Result<PathBuf, String> {
    use directories::ProjectDirs;

    ProjectDirs::from("com", "AZERBAIJAN-TECH", "SGLoader V2")
        .map(|dirs| dirs.data_dir().to_path_buf())
        .ok_or_else(|| "не удалось определить каталог данных пользователя".to_string())
}
