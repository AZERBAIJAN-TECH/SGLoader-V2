use dioxus::prelude::*;

use crate::storage::hub_urls;
use crate::ui::patches::{truncate_ellipsis, PatchesState};
use crate::{app_paths, marsey, settings};

#[component]
pub fn tab_settings(patches_state: Signal<PatchesState>) -> Element {
    #[derive(Clone, Copy, PartialEq)]
    enum SettingsTab {
        Patches,
        Game,
        Security,
    }

    let mut active_tab = use_signal(|| SettingsTab::Patches);

    let mut show_hub_settings = use_signal(|| false);
    let mut hub_list: Signal<Vec<String>> = use_signal(Vec::new);
    let mut hub_error: Signal<Option<String>> = use_signal(|| None::<String>);

    let mut game_error: Signal<Option<String>> = use_signal(|| None::<String>);
    let mut game_info: Signal<Option<String>> = use_signal(|| None::<String>);
    let mut game_cache_cleaning: Signal<bool> = use_signal(|| false);

    let mut launcher_settings: Signal<settings::LauncherSettings> =
        use_signal(settings::LauncherSettings::default);
    let mut settings_error: Signal<Option<String>> = use_signal(|| None::<String>);

    {
        let mut launcher_settings = launcher_settings;
        let mut settings_error = settings_error;
        use_future(move || async move {
            match settings::load_settings() {
                Ok(s) => {
                    settings_error.set(None);
                    launcher_settings.set(s);
                }
                Err(e) => {
                    settings_error.set(Some(e));
                }
            }
        });
    }

    let patches_state_value = patches_state();

    rsx! {
        div { class: "section settings-section",

            div { class: "filter-pills settings-tabs",
                button {
                    class: format_args!("pill {}", if active_tab() == SettingsTab::Patches { "active" } else { "" }),
                    onclick: move |_| active_tab.set(SettingsTab::Patches),
                    "Патчи"
                }
                button {
                    class: format_args!("pill {}", if active_tab() == SettingsTab::Game { "active" } else { "" }),
                    onclick: move |_| active_tab.set(SettingsTab::Game),
                    "Игра"
                }
                button {
                    class: format_args!("pill {}", if active_tab() == SettingsTab::Security { "active" } else { "" }),
                    onclick: move |_| active_tab.set(SettingsTab::Security),
                    "Безопасность"
                }
            }

            div { class: "settings-divider" }

            match active_tab() {
                SettingsTab::Patches => rsx! {
                    div { class: "patch-page",
                        div { class: "patch-actions",
                            button {
                                class: "ghost",
                                onclick: move |_| {
                                    patches_state.set(PatchesState::refresh());
                                },
                                "Обновить"
                            }
                            button {
                                class: "ghost",
                                onclick: move |_| {
                                    let Some(dir) = patches_state().mods_dir.clone() else {
                                        return;
                                    };
                                    let _ = crate::app_paths::open_in_file_manager(&dir);
                                },
                                "Директория патчей"
                            }
                        }

                        if let Some(err) = &patches_state_value.error {
                            p { class: "status status-error selectable", {err.clone()} }
                        }

                        div { class: "patch-header",
                            div { class: "patch-cell patch-cell-toggle" }
                            div { class: "patch-cell patch-cell-name", "Имя" }
                            div { class: "patch-cell patch-cell-desc", "Описание" }
                            div { class: "patch-cell patch-cell-rdnn", "RDNN" }
                        }

                        div { class: "patch-scroll",
                            if patches_state_value.patches.is_empty() {
                                p { class: "muted", "Патчи не найдены." }
                            } else {
                                div { class: "patch-rows",
                                    for patch in patches_state_value.patches.iter().cloned() {
                                        {
                                            let filename = patch.filename.clone();
                                            let checked = patch.enabled;
                                            let name = patch.name.clone();
                                            let desc = truncate_ellipsis(&patch.description, 100);
                                            let rdnn = patch.rdnn.clone();
                                            rsx! {
                                                div { class: "patch-row",
                                                    div { class: "patch-cell patch-cell-toggle",
                                                        input {
                                                            class: "patch-toggle",
                                                            r#type: "checkbox",
                                                            checked: checked,
                                                            onchange: move |_| {
                                                                let data_dir = match app_paths::data_dir() {
                                                                    Ok(dir) => dir,
                                                                    Err(e) => {
                                                                        patches_state.set(PatchesState { error: Some(e), ..patches_state() });
                                                                        return;
                                                                    }
                                                                };
                                                                let next = !checked;
                                                                if let Err(e) = marsey::set_patch_enabled(&data_dir, &filename, next) {
                                                                    patches_state.set(PatchesState { error: Some(e), ..patches_state() });
                                                                    return;
                                                                }
                                                                patches_state.set(PatchesState::refresh());
                                                            }
                                                        }
                                                    }
                                                    div { class: "patch-cell patch-cell-name", {name} }
                                                    div { class: "patch-cell patch-cell-desc", {desc} }
                                                    div { class: "patch-cell patch-cell-rdnn", {rdnn} }
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                },
                SettingsTab::Game => rsx! {
                    div { class: "patch-page",
                        div { class: "hub-actions",
                            button {
                                class: "ghost",
                                onclick: move |_| {
                                    hub_error.set(None);
                                    hub_list.set(hub_urls::load_hub_urls());
                                    show_hub_settings.set(true);
                                },
                                "Настройка хаба"
                            }

                            button {
                                class: "ghost",
                                disabled: game_cache_cleaning(),
                                onclick: move |_| {
                                    if game_cache_cleaning() {
                                        return;
                                    }

                                    game_cache_cleaning.set(true);
                                    game_error.set(None);
                                    game_info.set(Some("очистка...".to_string()));

                                    let mut game_error2 = game_error;
                                    let mut game_info2 = game_info;
                                    let mut game_cache_cleaning2 = game_cache_cleaning;
                                    spawn(async move {
                                        let data_dir = match app_paths::data_dir() {
                                            Ok(d) => d,
                                            Err(e) => {
                                                game_error2.set(Some(e));
                                                game_info2.set(None);
                                                game_cache_cleaning2.set(false);
                                                return;
                                            }
                                        };

                                        let res = tokio::task::spawn_blocking(move || {
                                            crate::core::cache_cleanup::clear_engines_cache(&data_dir)
                                        })
                                        .await;

                                        match res {
                                            Ok(Ok(())) => {
                                                game_error2.set(None);
                                                game_info2.set(Some("движки очищены".to_string()));
                                            }
                                            Ok(Err(e)) => {
                                                game_info2.set(None);
                                                game_error2.set(Some(e));
                                            }
                                            Err(e) => {
                                                game_info2.set(None);
                                                game_error2.set(Some(format!("ошибка задачи: {e}")));
                                            }
                                        }

                                        game_cache_cleaning2.set(false);
                                    });
                                },
                                "Очистить движки"
                            }

                            button {
                                class: "ghost",
                                disabled: game_cache_cleaning(),
                                onclick: move |_| {
                                    if game_cache_cleaning() {
                                        return;
                                    }

                                    game_cache_cleaning.set(true);
                                    game_error.set(None);
                                    game_info.set(Some("очистка...".to_string()));

                                    let mut game_error2 = game_error;
                                    let mut game_info2 = game_info;
                                    let mut game_cache_cleaning2 = game_cache_cleaning;
                                    spawn(async move {
                                        let data_dir = match app_paths::data_dir() {
                                            Ok(d) => d,
                                            Err(e) => {
                                                game_error2.set(Some(e));
                                                game_info2.set(None);
                                                game_cache_cleaning2.set(false);
                                                return;
                                            }
                                        };

                                        let res = tokio::task::spawn_blocking(move || {
                                            crate::core::cache_cleanup::clear_server_content_cache(&data_dir)
                                        })
                                        .await;

                                        match res {
                                            Ok(Ok(())) => {
                                                game_error2.set(None);
                                                game_info2.set(Some("контент серверов очищен".to_string()));
                                            }
                                            Ok(Err(e)) => {
                                                game_info2.set(None);
                                                game_error2.set(Some(e));
                                            }
                                            Err(e) => {
                                                game_info2.set(None);
                                                game_error2.set(Some(format!("ошибка задачи: {e}")));
                                            }
                                        }

                                        game_cache_cleaning2.set(false);
                                    });
                                },
                                "Очистить контент серверов"
                            }
                        }

                        if let Some(msg) = game_error() {
                            p { class: "status status-error selectable", {msg} }
                        } else if let Some(msg) = game_info() {
                            p { class: "status status-info", {msg} }
                        }
                    }

                    if show_hub_settings() {
                        HubSettingsModal {
                            urls: hub_list,
                            error: hub_error,
                            on_close: move |_| show_hub_settings.set(false),
                        }
                    }
                },
                SettingsTab::Security => rsx! {
                    div { class: "patch-page",
                        if let Some(msg) = settings_error() {
                            p { class: "status status-error selectable", {msg} }
                        }

                        div { class: "form",
                            label { "Уровень скрытия" }
                            select {
                                class: "select",
                                value: launcher_settings().security.hide_level.as_key(),
                                onchange: move |evt| {
                                    let Some(level) = settings::HideLevel::from_key(&evt.value()) else {
                                        return;
                                    };
                                    let mut next = launcher_settings();
                                    next.security.hide_level = level;
                                    match settings::save_settings(&next) {
                                        Ok(()) => settings_error.set(None),
                                        Err(e) => settings_error.set(Some(e)),
                                    }
                                    launcher_settings.set(next);
                                },
                                option {
                                    value: settings::HideLevel::Disabled.as_key(),
                                    selected: launcher_settings().security.hide_level == settings::HideLevel::Disabled,
                                    {settings::HideLevel::Disabled.label_ru()}
                                }
                                option {
                                    value: settings::HideLevel::Low.as_key(),
                                    selected: launcher_settings().security.hide_level == settings::HideLevel::Low,
                                    {settings::HideLevel::Low.label_ru()}
                                }
                                option {
                                    value: settings::HideLevel::Medium.as_key(),
                                    selected: launcher_settings().security.hide_level == settings::HideLevel::Medium,
                                    {settings::HideLevel::Medium.label_ru()}
                                }
                                option {
                                    value: settings::HideLevel::High.as_key(),
                                    selected: launcher_settings().security.hide_level == settings::HideLevel::High,
                                    {settings::HideLevel::High.label_ru()}
                                }
                                option {
                                    value: settings::HideLevel::Maximum.as_key(),
                                    selected: launcher_settings().security.hide_level == settings::HideLevel::Maximum,
                                    {settings::HideLevel::Maximum.label_ru()}
                                }
                            }

                            label { "Автоматический вход" }
                            div { class: "hub-row",
                                input {
                                    r#type: "checkbox",
                                    checked: launcher_settings().security.auto_login,
                                    onchange: move |_| {
                                        let mut next = launcher_settings();
                                        next.security.auto_login = !next.security.auto_login;
                                        match settings::save_settings(&next) {
                                            Ok(()) => settings_error.set(None),
                                            Err(e) => settings_error.set(Some(e)),
                                        }
                                        launcher_settings.set(next);
                                    }
                                }
                                span { class: "muted", "автоматически входить в аккаунт" }
                            }

                            label { "Redial" }
                            div { class: "hub-row",
                                input {
                                    r#type: "checkbox",
                                    checked: launcher_settings().security.disable_redial,
                                    onchange: move |_| {
                                        let mut next = launcher_settings();
                                        next.security.disable_redial = !next.security.disable_redial;
                                        match settings::save_settings(&next) {
                                            Ok(()) => settings_error.set(None),
                                            Err(e) => settings_error.set(Some(e)),
                                        }
                                        launcher_settings.set(next);
                                    }
                                }
                                span { class: "muted", "отключить переподключение к другим серверам" }
                            }

                            label { "HWID" }
                            div { class: "hub-row",
                                input {
                                    r#type: "checkbox",
                                    checked: launcher_settings().security.autodelete_hwid,
                                    onchange: move |_| {
                                        let mut next = launcher_settings();
                                        next.security.autodelete_hwid = !next.security.autodelete_hwid;
                                        match settings::save_settings(&next) {
                                            Ok(()) => settings_error.set(None),
                                            Err(e) => settings_error.set(Some(e)),
                                        }
                                        launcher_settings.set(next);
                                    }
                                }
                                span { class: "muted", "автоудаление HWID" }
                            }
                        }
                    }
                },
            }
        }
    }
}

#[component]
fn HubSettingsModal(
    urls: Signal<Vec<String>>,
    error: Signal<Option<String>>,
    on_close: EventHandler<()>,
) -> Element {
    let mut saving = use_signal(|| false);

    rsx! {
        div { class: "modal-backdrop",
            div { class: "modal hub-modal",
                div { class: "modal-header",
                    div {
                        h3 { "настройка хаба" }
                        p { class: "muted", "добавьте или уберите ссылки (http/https)" }
                    }
                }

                div { class: "modal-body",
                    div { class: "form",
                        label { "ссылки хаба" }

                        div { class: "hub-list",
                            for (idx, item) in urls().iter().cloned().enumerate() {
                                {
                                    let mut urls = urls;
                                    rsx! {
                                        div { class: "hub-row",
                                            input {
                                                r#type: "text",
                                                value: item,
                                                placeholder: "https://hub.example.com/",
                                                oninput: move |evt| {
                                                    let mut list = urls();
                                                    if idx < list.len() {
                                                        list[idx] = evt.value();
                                                        urls.set(list);
                                                    }
                                                }
                                            }
                                            button {
                                                class: "ghost",
                                                onclick: move |_| {
                                                    let mut list = urls();
                                                    if idx < list.len() {
                                                        list.remove(idx);
                                                        urls.set(list);
                                                    }
                                                },
                                                "Убрать"
                                            }
                                        }
                                    }
                                }
                            }
                        }

                        button {
                            class: "ghost",
                            onclick: move |_| {
                                let mut list = urls();
                                list.push(String::new());
                                urls.set(list);
                            },
                            "Добавить ссылку"
                        }
                    }

                    if let Some(msg) = error() {
                        p { class: "status status-error selectable", {msg} }
                    }
                }

                div { class: "modal-actions",
                    button {
                        class: "ghost",
                        disabled: saving(),
                        onclick: move |_| on_close.call(()),
                        "закрыть"
                    }
                    button {
                        class: "primary",
                        disabled: saving(),
                        onclick: move |_| {
                            if saving() {
                                return;
                            }

                            saving.set(true);
                            error.set(None);

                            let current = urls();
                            match hub_urls::save_hub_urls(&current) {
                                Ok(normalized) => {
                                    urls.set(normalized);
                                    saving.set(false);
                                    on_close.call(());
                                }
                                Err(e) => {
                                    saving.set(false);
                                    error.set(Some(e));
                                }
                            }
                        },
                        "сохранить"
                    }
                }
            }
        }
    }
}
