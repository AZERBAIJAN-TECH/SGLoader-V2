use std::fs;
use std::path::{Path, PathBuf};

pub fn clear_engines_cache(data_dir: &Path) -> Result<(), String> {
    clear_dir_if_exists(data_dir.join("engines"), "движки")
}

pub fn clear_server_content_cache(data_dir: &Path) -> Result<(), String> {
    clear_dir_if_exists(data_dir.join("content"), "контент серверов")?;
    clear_dir_if_exists(
        data_dir.join("content_overlay_cache"),
        "кэш оверлея контента",
    )?;
    clear_dir_if_exists(data_dir.join("content_blob_cache"), "blob cache контента")?;
    Ok(())
}

fn clear_dir_if_exists(path: PathBuf, label: &str) -> Result<(), String> {
    match fs::remove_dir_all(&path) {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(err) => Err(format!("не удалось очистить {label} ({:?}): {err}", path)),
    }
}
