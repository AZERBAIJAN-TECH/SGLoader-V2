use dioxus::prelude::*;

pub mod icons;
pub mod home;
pub mod news;
pub mod patches;
pub mod settings;
pub mod window;

use crate::account_store;
use crate::auth::{AuthApi, AuthenticateResult, LoginInfo};
use crate::constants::{APP_TITLE, STYLE};
use crate::ui::home::tab_home;
use crate::open_url;
use crate::ui::patches::PatchesState;
use crate::ui::news::tab_news;
use crate::ui::settings::tab_settings;

const DISCORD_INVITE_URL: &str = "https://discord.gg/HWvEa6KRYb";
const ACCOUNT_REGISTER_URL: &str = "https://account.spacestation14.com/Identity/Account/Register";

#[derive(Clone, Copy, PartialEq)]
enum Tab {
    Home,
    News,
    Settings,
}

pub fn app() -> Element {
    let auth_api = use_signal(AuthApi::new);
    let mut show_login = use_signal(|| true);
    let menu_open = use_signal(|| false);
    let mut active_account: Signal<Option<LoginInfo>> = use_signal(|| None);
    let saved_accounts: Signal<Vec<LoginInfo>> = use_signal(Vec::new);
    let mut active_tab = use_signal(|| Tab::Home);

    let patches_state: Signal<PatchesState> = use_signal(PatchesState::default);

    let mut toggle_menu = menu_open;
    let mut close_menu = menu_open;
    let active_account_sig = active_account;
    let mut login_open = show_login;
    let mut saved_accounts_sig = saved_accounts;
    let menu_state = menu_open;
    let current_account = active_account();
    let can_close_login = !saved_accounts().is_empty();

    {
        let mut saved_accounts = saved_accounts;
        use_future(move || async move {
            if let Ok(list) = account_store::load_saved_logins() {
                saved_accounts.set(list);
            }
        });
    }

    {
        let mut active_account = active_account;
        let mut show_login = show_login;
        use_future(move || async move {
            let allow_auto_login = crate::settings::load_settings()
                .ok()
                .map(|s| s.security.auto_login)
                .unwrap_or(true);

            if allow_auto_login && let Ok(Some(info)) = account_store::load_saved_login() {
                active_account.set(Some(info));
                show_login.set(false);
            }
        });
    }

    {
        let mut patches_state = patches_state;
        use_future(move || async move {
            patches_state.set(PatchesState::refresh());
        });
    }

    rsx! {
        Fragment {
            style { {STYLE} }
            div { class: "page",
                div { class: "card",
                    div { class: "title-row",
                        div { class: "title-left",
                            h1 { {APP_TITLE} }
                            p { class: "subtitle", "релиз" }
                        }
                        div { class: "title-right",
                            div { class: "title-right-links",
                                button {
                                    class: "pill discord-pill",
                                    onclick: move |_| open_url::open(DISCORD_INVITE_URL),
                                    DiscordIcon {}
                                    span { "Discord" }
                                }
                                span { class: "badge", "1.0.0-release" }
                            }
                        }
                    }

                    div { class: "tab-panel",
                        match active_tab() {
                            Tab::Home => rsx!(tab_home { active_account }),
                            Tab::News => rsx!(tab_news {}),
                            Tab::Settings => rsx!(tab_settings { patches_state }),
                        }
                    }

                    div { class: "tabs",
                        button {
                            class: format_args!("tab {}", if active_tab() == Tab::Home { "active" } else { "" }),
                            onclick: move |_| active_tab.set(Tab::Home),
                            "Home"
                        }
                        button {
                            class: format_args!("tab {}", if active_tab() == Tab::News { "active" } else { "" }),
                            onclick: move |_| active_tab.set(Tab::News),
                            "News"
                        }
                        button {
                            class: format_args!("tab {}", if active_tab() == Tab::Settings { "active" } else { "" }),
                            onclick: move |_| active_tab.set(Tab::Settings),
                            "Settings"
                        }

                        div { class: "tabs-spacer" }

                        div { class: "account-menu tabs-account",
                            button {
                                class: "tab tab-outline",
                                onclick: move |_| toggle_menu.set(!toggle_menu()),
                                {current_account.as_ref().map(|a| a.username.clone()).unwrap_or_else(|| "Войти".to_string())}
                            }

                            if menu_state() {
                                div { class: "dropdown up",
                                    for account in saved_accounts().into_iter() {
                                        {
                                            let account_id = account.user_id;
                                            let account_name = account.username.clone();
                                            let is_current = current_account
                                                .as_ref()
                                                .map(|cur| cur.user_id == account_id)
                                                .unwrap_or(false);
                                            let class_name = if is_current {
                                                "dropdown-item selected"
                                            } else {
                                                "dropdown-item"
                                            };

                                            let mut active_account_sig = active_account_sig;
                                            let mut close_menu = close_menu;
                                            let mut login_open = login_open;
                                            let mut saved_accounts_sig = saved_accounts_sig;
                                            let account_clone = account.clone();
                                            rsx! {
                                                button {
                                                    class: class_name,
                                                    onclick: move |_| {
                                                        close_menu.set(false);
                                                        let _ = account_store::set_active_login(account_id);
                                                        active_account_sig.set(Some(account_clone.clone()));
                                                        login_open.set(false);
                                                        if let Ok(list) = account_store::load_saved_logins() {
                                                            saved_accounts_sig.set(list);
                                                        }
                                                    },
                                                    {account_name}
                                                }
                                            }
                                        }
                                    }

                                    div { class: "dropdown-separator" }

                                    button {
                                        class: "dropdown-item",
                                        onclick: move |_| {
                                            close_menu.set(false);
                                            login_open.set(true);
                                        },
                                        "Добавить аккаунт"
                                    }

                                    if let Some(account) = current_account {
                                        {
                                            let user_id = account.user_id;
                                            let mut close_menu = close_menu;
                                            let mut active_account_sig = active_account_sig;
                                            let mut saved_accounts_sig = saved_accounts_sig;
                                            let mut login_open = login_open;
                                            rsx! {
                                                button {
                                                    class: "dropdown-item",
                                                    onclick: move |_| {
                                                        close_menu.set(false);
                                                        let before = saved_accounts_sig();
                                                        let removed_index = before.iter().position(|a| a.user_id == user_id);

                                                        let _ = account_store::remove_login(user_id);
                                                        let list = account_store::load_saved_logins().unwrap_or_default();
                                                        saved_accounts_sig.set(list.clone());

                                                        if list.is_empty() {
                                                            active_account_sig.set(None);
                                                            login_open.set(true);
                                                            return;
                                                        }

                                                        let mut pick_index = removed_index.unwrap_or(0);
                                                        if pick_index >= list.len() {
                                                            pick_index = list.len() - 1;
                                                        }

                                                        let picked = list[pick_index].clone();
                                                        let _ = account_store::set_active_login(picked.user_id);
                                                        active_account_sig.set(Some(picked));
                                                        login_open.set(false);
                                                    },
                                                    "Выйти"
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }

                if show_login() {
                    LoginOverlay {
                        auth_api: auth_api,
                        can_close: can_close_login,
                        on_success: move |info| {
                            let _ = account_store::save_login(&info);
                            if let Ok(list) = account_store::load_saved_logins() {
                                saved_accounts_sig.set(list);
                            }
                            active_account.set(Some(info));
                            show_login.set(false);
                        },
                        on_close: move |_| {
                            show_login.set(false);
                        }
                    }
                }
            }
        }
    }
}

#[component]
fn DiscordIcon() -> Element {
    rsx! {
        svg {
            class: "discord-icon",
            width: "14",
            height: "14",
            view_box: "0 0 127.14 127.14",
            xmlns: "http://www.w3.org/2000/svg",
            path {
                fill: "currentColor",
                d: "M107.7 23.46A105.15 105.15 0 0 0 81.47 15.39a72.06 72.06 0 0 0-3.36 6.83 97.68 97.68 0 0 0-29.11 0A72.37 72.37 0 0 0 45.64 15.39a105.89 105.89 0 0 0-26.25 8.09C2.79 48.04-1.71 71.99.54 95.6v0a105.73 105.73 0 0 0 32.17 16.15 77.7 77.7 0 0 0 6.89-11.11 68.42 68.42 0 0 1-10.85-5.18c.91-.66 1.8-1.34 2.66-2a75.57 75.57 0 0 0 64.32 0c.87.71 1.76 1.39 2.66 2a68.68 68.68 0 0 1-10.87 5.19 77 77 0 0 0 6.89 11.1A105.25 105.25 0 0 0 126.6 95.61v0c2.64-27.38-4.51-51.11-18.9-72.15ZM42.45 81.08C36.18 81.08 31 75.39 31 68.39c0-7 5-12.74 11.43-12.74 6.43 0 11.57 5.74 11.46 12.74-.11 7-5.05 12.69-11.44 12.69Zm42.24 0c-6.28 0-11.44-5.69-11.44-12.69 0-7 5-12.74 11.44-12.74 6.44 0 11.54 5.74 11.43 12.74-.11 7-5.04 12.69-11.43 12.69Z"
            }
        }
    }
}

#[component]
fn LoginOverlay(
    auth_api: Signal<AuthApi>,
    on_success: EventHandler<LoginInfo>,
    on_close: EventHandler<()>,
    can_close: bool,
) -> Element {
    let mut username = use_signal(String::new);
    let mut password = use_signal(String::new);
    let mut busy = use_signal(|| false);
    let mut error_message: Signal<Option<String>> = use_signal(|| None::<String>);

    let button_disabled = move || busy() || username().trim().is_empty() || password().is_empty();

    rsx! {
        div { class: "modal-backdrop locked",
            div { class: "modal login-modal",
                div { class: "modal-header",
                    div {
                        h3 { "авторизация" }
                        p { class: "muted", "введите данные учетной записи" }
                    }
                }

                div { class: "modal-body",
                    div { class: "form",
                        label { "имя пользователя" }
                        input {
                            r#type: "text",
                            value: username(),
                            placeholder: "username",
                            oninput: move |evt| username.set(evt.value())
                        }

                        label { "пароль" }
                        input {
                            r#type: "password",
                            value: password(),
                            placeholder: "********",
                            oninput: move |evt| password.set(evt.value())
                        }
                    }

                    if let Some(message) = error_message() {
                        p { class: "status status-error", {message} }
                    }
                }

                div { class: "modal-actions",
                    button {
                        class: "ghost modal-actions-left",
                        onclick: move |_| open_url::open(ACCOUNT_REGISTER_URL),
                        "создать аккаунт"
                    }
                    button {
                        class: "ghost",
                        disabled: busy() || !can_close,
                        onclick: move |_| {
                            if !can_close {
                                return;
                            }
                            on_close.call(());
                        },
                        "закрыть"
                    }
                    button {
                        class: "primary",
                        disabled: button_disabled(),
                        onclick: move |_| {
                            if busy() {
                                return;
                            }

                            let user = username().trim().to_string();
                            let pass = password();

                            if user.is_empty() || pass.is_empty() {
                                error_message.set(Some("введите имя пользователя и пароль".to_string()));
                                return;
                            }

                            busy.set(true);
                            error_message.set(None);

                            let api = auth_api();
                            let mut busy_done = busy;
                            let mut error_done: Signal<Option<String>> = error_message;
                            let success_cb = on_success;

                            spawn(async move {
                                match api.authenticate(user, pass).await {
                                    Ok(AuthenticateResult::Success(info)) => {
                                        success_cb.call(info);
                                    }
                                    Ok(AuthenticateResult::Failure { errors, code }) => {
                                        let message = if errors.is_empty() {
                                            format!("ошибка: {:?}", code)
                                        } else {
                                            errors.join("\n")
                                        };
                                        error_done.set(Some(message));
                                    }
                                    Err(err) => {
                                        error_done.set(Some(err.to_string()));
                                    }
                                }

                                busy_done.set(false);
                            });
                        },
                        {if busy() { "входим..." } else { "войти" }}
                    }
                }
            }
        }
    }
}
