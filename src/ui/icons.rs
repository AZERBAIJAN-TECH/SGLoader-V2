use std::path::{Path, PathBuf};

use dioxus_desktop::tao::window::Icon;

use crate::constants::ASSETS_DIR;

pub fn load_icon(file_name: &str) -> Option<Icon> {
    for path in icon_search_paths(file_name) {
        if let Ok(icon) = load_icon_from_file(&path) {
            return Some(icon);
        }
    }

    None
}

fn load_icon_from_file(path: &Path) -> Result<Icon, Box<dyn std::error::Error>> {
    let data = std::fs::read(path)?;
    let dyn_img = image::load_from_memory(&data)?;
    let rgba = dyn_img.to_rgba8();
    let (width, height) = rgba.dimensions();
    Ok(Icon::from_rgba(rgba.into_raw(), width, height)?)
}

fn icon_search_paths(file_name: &str) -> Vec<PathBuf> {
    let mut paths = Vec::with_capacity(2);

    if let Ok(exe_dir) = std::env::current_exe()
        && let Some(parent) = exe_dir.parent()
    {
        paths.push(parent.join(ASSETS_DIR).join(file_name));
    }

    paths.push(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join(ASSETS_DIR)
            .join(file_name),
    );

    paths
}
