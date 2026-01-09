use dioxus_desktop::tao::window::Icon;
use dioxus_desktop::{Config, LogicalSize, WindowBuilder};

use crate::constants::{APP_TITLE, TASKBAR_ICON, TITLEBAR_ICON, WINDOW_SIZE};
use crate::ui::icons::load_icon;

pub fn app_window() -> Config {
    let (width, height) = WINDOW_SIZE;
    let titlebar_icon = load_icon(TITLEBAR_ICON);
    let taskbar_icon = load_icon(TASKBAR_ICON);

    let builder = WindowBuilder::new()
        .with_title(APP_TITLE)
        .with_decorations(true)
        .with_window_icon(titlebar_icon)
        .with_inner_size(LogicalSize::new(width, height))
        .with_min_inner_size(LogicalSize::new(width, height))
        .with_resizable(true);

    let builder = apply_taskbar_icon(builder, taskbar_icon);

    Config::default()
        .with_menu(None)
        .with_disable_context_menu(true)
        .with_window(builder)
}

#[cfg(target_os = "windows")]
fn apply_taskbar_icon(builder: WindowBuilder, taskbar_icon: Option<Icon>) -> WindowBuilder {
    use dioxus_desktop::tao::platform::windows::WindowBuilderExtWindows;
    builder.with_taskbar_icon(taskbar_icon)
}

#[cfg(not(target_os = "windows"))]
fn apply_taskbar_icon(builder: WindowBuilder, _taskbar_icon: Option<Icon>) -> WindowBuilder {
    builder
}
