use std::collections::HashSet;
use std::fs;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

const FAVORITES_FILE_NAME: &str = "favorites.json";

pub fn load_favorites() -> Result<HashSet<String>, String> {
    let path = favorites_file_path()?;
    let contents = match fs::read_to_string(&path) {
        Ok(data) => data,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(HashSet::new()),
        Err(err) => return Err(format!("не удалось прочитать избранное: {err}")),
    };

    let stored: FavoritesFile = serde_json::from_str(&contents)
        .map_err(|e| format!("не удалось разобрать избранное: {e}"))?;

    Ok(stored.addresses.into_iter().collect())
}

pub fn save_favorites(set: &HashSet<String>) -> Result<(), String> {
    let dir = crate::app_paths::data_dir()?;
    fs::create_dir_all(&dir).map_err(|e| format!("mkdir избранное: {e}"))?;

    let path = favorites_file_path()?;
    let mut addresses: Vec<String> = set.iter().cloned().collect();
    addresses.sort();

    let stored = FavoritesFile { addresses };
    let json =
        serde_json::to_string_pretty(&stored).map_err(|e| format!("serialize избранное: {e}"))?;

    fs::write(&path, json).map_err(|e| format!("запись избранного: {e}"))?;
    Ok(())
}

fn favorites_file_path() -> Result<PathBuf, String> {
    Ok(crate::app_paths::data_dir()?.join(FAVORITES_FILE_NAME))
}

#[derive(Debug, Serialize, Deserialize, Default)]
struct FavoritesFile {
    addresses: Vec<String>,
}

pub fn canonicalize_favorite_address(address: &str) -> String {
    // Keep the exact address the server list provides, but normalize whitespace.
    address.trim().to_string()
}

pub fn is_favorite(set: &HashSet<String>, address: &str) -> bool {
    set.contains(&canonicalize_favorite_address(address))
}

pub fn toggle_favorite(set: &mut HashSet<String>, address: &str) {
    let addr = canonicalize_favorite_address(address);
    if !set.insert(addr.clone()) {
        set.remove(&addr);
    }
}

pub fn data_dir_path_for_debug() -> Result<PathBuf, String> {
    crate::app_paths::data_dir()
}
