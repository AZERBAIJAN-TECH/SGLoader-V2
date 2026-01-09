use std::path::PathBuf;

use crate::{app_paths, marsey};

#[derive(Clone, Debug, PartialEq)]
pub struct PatchRow {
    pub filename: String,
    pub enabled: bool,
    pub name: String,
    pub description: String,
    pub rdnn: String,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct PatchesState {
    pub mods_dir: Option<PathBuf>,
    pub patches: Vec<PatchRow>,
    pub error: Option<String>,
}

impl PatchesState {
    pub fn refresh() -> Self {
        let data_dir = match app_paths::data_dir() {
            Ok(dir) => dir,
            Err(e) => {
                return Self {
                    error: Some(e),
                    ..Default::default()
                };
            }
        };

        match marsey::list_patches(&data_dir) {
            Ok((mods_dir, entries)) => {
                let patches = entries
                    .into_iter()
                    .map(|p| PatchRow {
                        filename: p.filename,
                        enabled: p.enabled,
                        name: p.name,
                        description: p.description,
                        rdnn: p.rdnn,
                    })
                    .collect();

                Self {
                    mods_dir: Some(mods_dir),
                    patches,
                    error: None,
                }
            }
            Err(e) => Self {
                error: Some(e),
                ..Default::default()
            },
        }
    }
}

pub fn truncate_ellipsis(input: &str, max_chars: usize) -> String {
    let count = input.chars().count();
    if count <= max_chars {
        return input.to_string();
    }

    let mut out: String = input.chars().take(max_chars).collect();
    out.push_str("...");
    out
}
