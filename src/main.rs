#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod core;
mod install;
mod marsey;
mod net;
mod ss14;
mod storage;
mod ui;

pub use core::cache_cleanup;
pub use core::open_url;
pub use core::{app_paths, cancel_flag, constants};
pub use install::{acz_content, client_install, content_install, launcher_mask, robust_builds};
pub use net::{auth, connect, connect_progress, http_config, servers};
pub use ss14::{ss14_loader, ss14_server_info, ss14_uri};
pub use storage::{account_store, favorites, secure_token, settings};

pub use marsey::*;

pub use ui::{home, icons, news, window};

use dioxus::prelude::*;

use crate::ui::app;
use crate::window::app_window;

fn main() {
    LaunchBuilder::desktop().with_cfg(app_window()).launch(app);
}
