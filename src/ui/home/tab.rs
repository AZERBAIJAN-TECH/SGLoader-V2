use std::collections::HashSet;
use std::time::{Duration, Instant};

use dioxus::prelude::*;

use crate::auth::LoginInfo;
use crate::cancel_flag::CancelFlag;
use crate::connect_progress::ConnectProgress;
use crate::favorites;
use crate::servers::{fetch_server_description, fetch_server_list, ServerEntry};

use super::helpers::{display_region, display_tag, truncate_name};

#[component]
pub fn tab_home(active_account: Signal<Option<LoginInfo>>) -> Element {
    let servers = use_signal(Vec::<ServerEntry>::new);
    let loading = use_signal(|| true);
    let error_message: Signal<Option<String>> = use_signal(|| None);
    let mut connect_message: Signal<Option<String>> = use_signal(|| None);
    let connect_stage: Signal<String> = use_signal(|| "".to_string());
    let connect_download_label: Signal<Option<String>> = use_signal(|| None);
    let connect_done_bytes: Signal<u64> = use_signal(|| 0);
    let connect_total_bytes: Signal<Option<u64>> = use_signal(|| None);
    let connect_logs: Signal<Vec<String>> = use_signal(Vec::<String>::new);
    let connect_cancel: Signal<Option<CancelFlag>> = use_signal(|| None);
    let connecting = use_signal(|| false);
    let mut show_connect_modal = use_signal(|| false);

    let connect_success = use_signal(|| false);
    let game_launched_at: Signal<Option<Instant>> = use_signal(|| None);
    let mut last_launcher_activity_at: Signal<Instant> = use_signal(Instant::now);

    let mut search = use_signal(String::new);
    let mut region = use_signal(|| "all".to_string());
    let mut only_online = use_signal(|| false);
    let mut hide_full = use_signal(|| false);
    let mut hide_empty = use_signal(|| false);
    let mut min_players = use_signal(|| 0u32);
    let mut max_players = use_signal(|| None::<u32>);
    let mut selected_langs = use_signal(Vec::<String>::new);
    let mut selected_rp = use_signal(Vec::<String>::new);
    let mut sort_mode = use_signal(|| "online_desc".to_string());
    let mut show_filters = use_signal(|| false);
    let mut show_direct_connect = use_signal(|| false);
    let mut direct_connect_address = use_signal(String::new);
    let mut direct_connect_error: Signal<Option<String>> = use_signal(|| None);
    let expanded_desc = use_signal(HashSet::<String>::new);
    let favorites_set = use_signal(HashSet::<String>::new);

    {
        let mut servers = servers;
        let mut loading = loading;
        let mut error_message = error_message;
        use_future(move || async move {
            loading.set(true);
            match fetch_server_list().await {
                Ok(list) => {
                    servers.set(list);
                    error_message.set(None);
                }
                Err(err) => error_message.set(Some(err)),
            }
            loading.set(false);
        });
    }

    {
        let mut fav_sig = favorites_set;
        use_future(move || async move {
            if let Ok(set) = favorites::load_favorites() {
                fav_sig.set(set);
            }
        });
    }

    let regions: Vec<String> = {
        let mut list: Vec<String> = servers().iter().filter_map(|s| s.region.clone()).collect();
        list.sort();
        list.dedup();
        list
    };

    let (filtered_servers, favorite_count): (Vec<(ServerEntry, String, String)>, usize) = {
        let needle = search().to_lowercase();
        let selected_region = region();
        let langs = selected_langs();
        let rp_levels = selected_rp();
        let min_players = min_players();
        let max_players = max_players();
        let mut list: Vec<ServerEntry> = servers()
            .into_iter()
            .filter(|srv| {
                let matches_search = needle.is_empty()
                    || srv.name.to_lowercase().contains(&needle)
                    || srv.address.to_lowercase().contains(&needle)
                    || srv
                        .tags
                        .iter()
                        .any(|tag| tag.to_lowercase().contains(&needle));

                let matches_region = selected_region == "all"
                    || srv.region.as_deref() == Some(selected_region.as_str());
                let matches_online = !only_online() || srv.online;
                let matches_full = !hide_full() || srv.players < srv.max_players;
                let matches_empty = !hide_empty() || srv.players > 0;

                let matches_lang = if langs.is_empty() {
                    true
                } else {
                    langs.iter().any(|code| {
                        srv.tags
                            .iter()
                            .any(|t| t.to_lowercase() == format!("lang:{}", code))
                    })
                };

                let matches_rp = if rp_levels.is_empty() {
                    true
                } else {
                    rp_levels.iter().any(|lvl| {
                        srv.tags
                            .iter()
                            .any(|t| t.to_lowercase() == format!("rp:{}", lvl))
                    })
                };

                let matches_min = srv.players >= min_players;
                let matches_max = max_players.map(|m| srv.players <= m).unwrap_or(true);

                matches_search
                    && matches_region
                    && matches_online
                    && matches_full
                    && matches_empty
                    && matches_lang
                    && matches_rp
                    && matches_min
                    && matches_max
            })
            .collect();

        match sort_mode().as_str() {
            "online_desc" => list.sort_by(|a, b| b.players.cmp(&a.players)),
            "online_asc" => list.sort_by(|a, b| a.players.cmp(&b.players)),
            "name_asc" => list.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase())),
            "name_desc" => list.sort_by(|a, b| b.name.to_lowercase().cmp(&a.name.to_lowercase())),
            _ => {}
        }

        let favs = favorites_set();
        let mut fav_list: Vec<(ServerEntry, String, String)> = Vec::new();
        let mut other_list: Vec<(ServerEntry, String, String)> = Vec::new();

        for srv in list.into_iter() {
            let addr = srv.address.clone();
            let addr_fav = addr.clone();
            let fav_key = favorites::canonicalize_favorite_address(&addr_fav);
            if favs.contains(&fav_key) {
                fav_list.push((srv, addr, addr_fav));
            } else {
                other_list.push((srv, addr, addr_fav));
            }
        }

        let favorite_count = fav_list.len();
        fav_list.extend(other_list);
        (fav_list, favorite_count)
    };

    let filtered_servers_len = filtered_servers.len();

    let mut reset_filters = move || {
        search.set(String::new());
        region.set("all".to_string());
        only_online.set(false);
        hide_full.set(false);
        hide_empty.set(false);
        min_players.set(0);
        max_players.set(None);
        selected_langs.set(Vec::new());
        selected_rp.set(Vec::new());
    };

    let regions_list = regions.clone();

    rsx! {
        div {
            class: "section",
            onmousedown: move |_| last_launcher_activity_at.set(Instant::now()),
            onmousemove: move |_| last_launcher_activity_at.set(Instant::now()),
            onkeydown: move |_| last_launcher_activity_at.set(Instant::now()),
            p { class: "muted", {format!("Серверов: {}", servers().len())} }

            div { class: "filter-bar",
                button {
                    class: "pill ghost",
                    onclick: move |_| {
                        direct_connect_error.set(None);
                        show_direct_connect.set(true);
                    },
                    "Прямое подключение"
                }

                button {
                    class: "pill ghost",
                    onclick: move |_| show_filters.set(true),
                    "Фильтры"
                }

                input {
                    class: "input text-input",
                    r#type: "search",
                    placeholder: "Поиск по названию/Адресу",
                    value: search(),
                    oninput: move |evt| search.set(evt.value()),
                }

                select {
                    class: "select sort-select",
                    value: sort_mode(),
                    oninput: move |evt| sort_mode.set(evt.value()),
                    option { value: "online_desc", "Сортировать: онлайн ↓" }
                    option { value: "online_asc", "Сортировать: онлайн ↑" }
                    option { value: "name_asc", "Сортировать: А→Я" }
                    option { value: "name_desc", "Сортировать: Я→А" }
                }
            }

            if loading() {
                p { class: "status status-info", "загружаем список серверов..." }
            }

            if let Some(err) = error_message() {
                 div { class: "status status-error status-block selectable error-log", {format!("ошибка: {}", err)} }
            }

            if show_connect_modal() {
                div { class: "modal-backdrop locked",
                    div {
                        class: "modal login-modal connect-modal",
                        onmousedown: move |_| last_launcher_activity_at.set(Instant::now()),
                        onmousemove: move |_| last_launcher_activity_at.set(Instant::now()),
                        onkeydown: move |_| last_launcher_activity_at.set(Instant::now()),
                        div { class: "modal-header",
                            div {
                                h3 { "подключение" }
                                p { class: "muted",
                                    { if connecting() { "подключаемся к серверу" } else { "готово" } }
                                }
                            }
                        }

                        div { class: "modal-body",
                            if !connect_stage().is_empty() {
                                p { class: "muted", {connect_stage()} }
                            }

                            if let Some(label) = connect_download_label() {
                                {
                                    let done = connect_done_bytes();
                                    let total = connect_total_bytes();
                                    rsx! {
                                        div { class: "connect-progress",
                                            p { class: "muted", {format!("{}: {}{}", label, format_bytes(done), total.map(|t| format!(" / {}", format_bytes(t))).unwrap_or_default())} }

                                            // Always show an indeterminate (cyclic) progress bar.
                                            div { class: "progress-indeterminate",
                                                div { class: "progress-indeterminate-bar" }
                                            }
                                        }
                                    }
                                }
                            }

                            if !connect_logs().is_empty() {
                                div { class: "status status-info status-block selectable connect-log",
                                    {connect_logs().join("\n")}
                                }
                            }

                            if let Some(msg) = connect_message() {
                                div { class: "status status-info status-block selectable", {msg} }
                            } else {
                                p { class: "muted", "ожидание..." }
                            }
                        }

                        div { class: "modal-actions",
                            button {
                                class: "ghost",
                                onclick: move |_| {
                                    if connecting() {
                                        if let Some(flag) = connect_cancel() {
                                            flag.cancel();
                                            connect_message.set(Some("отменяем...".to_string()));
                                        }
                                        // Allow the user to dismiss the modal even if the
                                        // background connect task is still unwinding.
                                        show_connect_modal.set(false);
                                        return;
                                    }

                                    show_connect_modal.set(false);
                                },
                                { if connecting() { "остановить" } else { "закрыть" } }
                            }
                        }
                    }
                }
            }

            if show_direct_connect() {
                div { class: "modal-backdrop", onclick: move |_| show_direct_connect.set(false),
                    div { class: "modal filter-modal", onclick: move |evt| evt.stop_propagation(),
                        div { class: "modal-header",
                            h3 { "Прямое подключение" }
                        }
                        div { class: "modal-body",
                            p { class: "muted",
                                "Ссылка на сервер"
                            }
                            input {
                                class: "input text-input",
                                r#type: "text",
                                placeholder: "ss14://127.0.0.1:1212",
                                value: direct_connect_address(),
                                oninput: move |evt| {
                                    direct_connect_address.set(evt.value());
                                    direct_connect_error.set(None);
                                },
                            }
                            if let Some(err) = direct_connect_error() {
                                div { class: "status status-error status-block selectable", {err} }
                            }
                        }
                        div { class: "modal-actions",
                            button {
                                class: "ghost",
                                onclick: move |_| show_direct_connect.set(false),
                                "Закрыть"
                            }
                            button {
                                class: "primary",
                                disabled: connecting() || direct_connect_address().trim().is_empty(),
                                onclick: move |_| {
                                    let input = direct_connect_address().trim().to_string();
                                    if input.is_empty() {
                                        direct_connect_error.set(Some("введите адрес сервера".to_string()));
                                        return;
                                    }

                                    match crate::ss14_uri::parse_ss14_uri(&input) {
                                        Ok(uri) => {
                                            direct_connect_error.set(None);
                                            show_direct_connect.set(false);
                                            start_connect_task(
                                                uri.to_string(),
                                                active_account(),
                                                connecting,
                                                show_connect_modal,
                                                connect_message,
                                                connect_stage,
                                                connect_download_label,
                                                connect_done_bytes,
                                                connect_total_bytes,
                                                connect_logs,
                                                connect_cancel,
                                                connect_success,
                                                game_launched_at,
                                                last_launcher_activity_at,
                                            );
                                        }
                                        Err(e) => direct_connect_error.set(Some(e)),
                                    }
                                },
                                "Подключиться"
                            }
                        }
                    }
                }
            }

            if show_filters() {
                div { class: "modal-backdrop", onclick: move |_| show_filters.set(false),
                    div { class: "modal filter-modal", onclick: move |evt| evt.stop_propagation(),
                        div { class: "modal-header",
                            h3 { "Фильтры" }
                        }
                        div { class: "modal-body filters-body",
                            div { class: "filters-group",
                                h4 { "Язык" }
                                {
                                    let mut langs_sig = selected_langs;
                                    let current_lang = selected_langs()
                                        .first()
                                        .cloned()
                                        .unwrap_or_else(|| "all".to_string());
                                    rsx! {
                                        select {
                                            class: "select",
                                            value: current_lang,
                                            oninput: move |evt| {
                                                let val = evt.value();
                                                if val == "all" {
                                                    langs_sig.set(Vec::new());
                                                } else {
                                                    langs_sig.set(vec![val]);
                                                }
                                            },
                                            option { value: "all", "Все языки" }
                                            option { value: "en", "English" }
                                            option { value: "ru", "Русский" }
                                            option { value: "fr", "French" }
                                            option { value: "de", "German" }
                                            option { value: "pl", "Polish" }
                                            option { value: "pt", "Portuguese" }
                                            option { value: "uk", "Ukrainian" }
                                        }
                                    }
                                }
                            }

                            div { class: "filters-group",
                                h4 { "Показ" }
                                div { class: "chips",
                                    {
                                        let mut only_online_sig = only_online;
                                        rsx! {
                                            button {
                                                class: format_args!("pill chip {}", if only_online() { "active" } else { "" }),
                                                onclick: move |_| only_online_sig.set(!only_online_sig()),
                                                {if only_online() { "только онлайн" } else { "все" }}
                                            }
                                        }
                                    }
                                    {
                                        let mut hide_full_sig = hide_full;
                                        rsx! {
                                            button {
                                                class: format_args!("pill chip {}", if hide_full() { "active" } else { "" }),
                                                onclick: move |_| hide_full_sig.set(!hide_full_sig()),
                                                "без заполненных"
                                            }
                                        }
                                    }
                                    {
                                        let mut hide_empty_sig = hide_empty;
                                        rsx! {
                                            button {
                                                class: format_args!("pill chip {}", if hide_empty() { "active" } else { "" }),
                                                onclick: move |_| hide_empty_sig.set(!hide_empty_sig()),
                                                "без пустых"
                                            }
                                        }
                                    }
                                }
                            }

                            div { class: "filters-group",
                                h4 { "Регион" }
                                div { class: "chips",
                                    for reg in std::iter::once("all".to_string()).chain(regions_list.clone()) {
                                        {
                                            let reg_owned = reg.clone();
                                            let is_all = reg_owned == "all";
                                            let active = region() == reg_owned;
                                            let mut region_sig = region;
                                            let label = if is_all {
                                                "все".to_string()
                                            } else {
                                                display_region(&reg_owned).to_lowercase()
                                            };
                                            rsx! {
                                                button {
                                                    class: format_args!("pill chip {}", if active { "active" } else { "" }),
                                                    onclick: move |_| region_sig.set(reg_owned.clone()),
                                                    {label}
                                                }
                                            }
                                        }
                                    }
                                }
                            }

                            div { class: "filters-group",
                                h4 { "RP-уровень" }
                                div { class: "chips",
                                    for (code, label_text) in [("low", "LRP"), ("med", "MRP"), ("high", "HRP")] {
                                        {
                                            let code_owned = code.to_string();
                                            let active = selected_rp().contains(&code_owned);
                                            let mut selected_rp_sig = selected_rp;
                                            rsx! {
                                                button {
                                                    class: format_args!("pill chip {}", if active { "active" } else { "" }),
                                                    onclick: move |_| {
                                                        let mut list = selected_rp_sig();
                                                        if let Some(pos) = list.iter().position(|c| c == &code_owned) {
                                                            list.remove(pos);
                                                        } else {
                                                            list.push(code_owned.clone());
                                                        }
                                                        selected_rp_sig.set(list);
                                                    },
                                                    {label_text}
                                                }
                                            }
                                        }
                                    }
                                }
                            }

                            div { class: "filters-group two-cols",
                                div { class: "field",
                                    label { "Мин. игроков" }
                                    input {
                                        class: "input",
                                        r#type: "number",
                                        min: "0",
                                        value: format!("{}", min_players()),
                                        oninput: move |evt| {
                                            if let Ok(val) = evt.value().parse::<u32>() {
                                                min_players.set(val);
                                            }
                                        },
                                    }
                                }
                                div { class: "field",
                                    label { "Макс. игроков" }
                                    input {
                                        class: "input",
                                        r#type: "number",
                                        min: "0",
                                        value: max_players().map(|v| v.to_string()).unwrap_or_else(|| "".to_string()),
                                        placeholder: "нет",
                                        oninput: move |evt| {
                                            let txt = evt.value();
                                            if txt.is_empty() {
                                                max_players.set(None);
                                            } else if let Ok(val) = txt.parse::<u32>() {
                                                max_players.set(Some(val));
                                            }
                                        },
                                    }
                                }
                            }
                        }
                        div { class: "modal-actions",
                            button { class: "ghost", onclick: move |_| reset_filters(), "Сбросить" }
                            button { class: "primary", onclick: move |_| show_filters.set(false), "Готово" }
                        }
                    }
                }
            }

            div { class: "server-list compact",
                if !loading() && filtered_servers.is_empty() {
                    div { class: "empty-state",
                        h3 { "Ничего не нашли" }
                        p { class: "muted", "Попробуй изменить фильтры или строку поиска." }
                    }
                } else {
                    for (i, (server, addr_connect, addr_fav)) in filtered_servers.into_iter().enumerate() {
                        if i == favorite_count && favorite_count > 0 && favorite_count < filtered_servers_len {
                            div { class: "settings-divider" }
                        }
                        {
                            let key = addr_connect.clone();
                            let expanded = expanded_desc().contains(&key);
                            let mut expanded_sig = expanded_desc;
                            let servers_sig = servers;
                            let needs_desc_fetch = server.description.is_none();
                            let addr_connect_for_desc = addr_connect.clone();
                            let fav_key = favorites::canonicalize_favorite_address(&addr_fav);
                            let is_fav = favorites_set().contains(&fav_key);
                            let mut fav_sig = favorites_set;
                            rsx! {
                                div { key: "{addr_connect}", class: "server-card row",
                                    div { class: "server-row",
                                        div { class: "server-main",
                                            div { class: "server-name-block",
                                                div { class: "name-line",
                                                    h3 { title: server.name.clone(), {truncate_name(&server.name, 100)} }
                                                    if let Some(region) = server.region.clone() {
                                                            span { class: "region-pill", {display_region(&region)} }
                                                    }
                                                }

                                                if !server.tags.is_empty() {
                                                    div { class: "tag-row dense",
                                                            for tag in server.tags.iter() {
                                                                if let Some(label) = display_tag(tag) {
                                                                    span { class: "tag", {label} }
                                                                }
                                                        }
                                                    }
                                                }
                                            }
                                        }

                                        div { class: "server-right",
                                            div { class: "stat-line",
                                                span { class: "stat players", {format!("{}/{}", server.players, server.max_players)} }
                                                span { class: "stat ping", {server.ping_ms.map(|p| format!("{} мс", p)).unwrap_or_else(|| "—".to_string())} }
                                            }

                                            div { class: "server-actions",
                                                button {
                                                    class: "primary small",
                                                    disabled: !server.online || connecting(),
                                                    onclick: move |_| {
                                                        start_connect_task(
                                                            addr_connect.clone(),
                                                            active_account(),
                                                            connecting,
                                                            show_connect_modal,
                                                            connect_message,
                                                            connect_stage,
                                                            connect_download_label,
                                                            connect_done_bytes,
                                                            connect_total_bytes,
                                                            connect_logs,
                                                            connect_cancel,
                                                            connect_success,
                                                            game_launched_at,
                                                            last_launcher_activity_at,
                                                        );
                                                    },
                                                    "Подключиться"
                                                }

                                                button {
                                                    class: format_args!("ghost small {}", if expanded { "active" } else { "" }),
                                                    onclick: move |_| {
                                                        let mut set = expanded_sig();
                                                        let expanding = !set.contains(&key);
                                                        if !expanding {
                                                            set.remove(&key);
                                                        } else {
                                                            set.insert(key.clone());
                                                        }
                                                        expanded_sig.set(set);

                                                        if expanding && needs_desc_fetch {
                                                            let mut servers_sig2 = servers_sig;
                                                            let address = addr_connect_for_desc.clone();
                                                            spawn(async move {
                                                                match fetch_server_description(&address).await {
                                                                    Ok(desc) => {
                                                                        let mut list = servers_sig2();
                                                                        if let Some(srv) = list.iter_mut().find(|s| s.address == address) {
                                                                            srv.description = Some(
                                                                                desc.unwrap_or_else(|| "Описание не указано".to_string()),
                                                                            );
                                                                            servers_sig2.set(list);
                                                                        }
                                                                    }
                                                                    Err(_) => {}
                                                                }
                                                            });
                                                        }
                                                    },
                                                    { if expanded { "Скрыть описание" } else { "Описание" } }
                                                }

                                                button {
                                                    class: "ghost small",
                                                    onclick: move |_| {
                                                        let mut set = fav_sig();
                                                        favorites::toggle_favorite(&mut set, &addr_fav);
                                                        fav_sig.set(set.clone());

                                                        spawn(async move {
                                                            let _ = tokio::task::spawn_blocking(move || favorites::save_favorites(&set)).await;
                                                        });
                                                    },
                                                    { if is_fav { "В избранном" } else { "В избранное" } }
                                                }
                                            }
                                        }
                                    }

                                    if expanded {
                                        div { class: "server-description", { server.description.clone().unwrap_or_else(|| "Описание недоступно".to_string()) } }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

fn start_connect_task(
    address: String,
    account: Option<LoginInfo>,
    mut connecting: Signal<bool>,
    mut show_connect_modal: Signal<bool>,
    mut connect_message: Signal<Option<String>>,
    mut connect_stage: Signal<String>,
    mut connect_download_label: Signal<Option<String>>,
    mut connect_done_bytes: Signal<u64>,
    mut connect_total_bytes: Signal<Option<u64>>,
    mut connect_logs: Signal<Vec<String>>,
    mut connect_cancel: Signal<Option<CancelFlag>>,
    mut connect_success: Signal<bool>,
    mut game_launched_at: Signal<Option<Instant>>,
    last_launcher_activity_at: Signal<Instant>,
) {
    if connecting() {
        return;
    }

    connecting.set(true);
    show_connect_modal.set(true);

    connect_message.set(Some(format!("подключаемся к {}...", address)));
    connect_stage.set("подготовка...".to_string());
    connect_download_label.set(None);
    connect_done_bytes.set(0);
    connect_total_bytes.set(None);
    connect_logs.set(Vec::new());

    connect_success.set(false);
    game_launched_at.set(None);

    let cancel_flag = CancelFlag::new();
    connect_cancel.set(Some(cancel_flag.clone()));

    spawn(async move {
        let mut msg_sig = connect_message;
        let mut cancel_sig = connect_cancel;
        let mut connecting_sig = connecting;
        let mut connect_success_sig = connect_success;

        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<ConnectProgress>();

        let mut stage_sig2 = connect_stage;
        let mut label_sig2 = connect_download_label;
        let mut done_sig2 = connect_done_bytes;
        let mut total_sig2 = connect_total_bytes;
        let mut logs_sig2 = connect_logs;

        let mut game_launched_at_sig2 = game_launched_at;
        let show_connect_modal_sig2 = show_connect_modal;
        let connect_success_sig2 = connect_success_sig;
        let connecting_sig2 = connecting_sig;
        let last_activity_sig2 = last_launcher_activity_at;
        spawn(async move {
            while let Some(ev) = rx.recv().await {
                match ev {
                    ConnectProgress::Stage(s) => stage_sig2.set(s),
                    ConnectProgress::Download {
                        label,
                        done_bytes,
                        total_bytes,
                    } => {
                        label_sig2.set(Some(label));
                        done_sig2.set(done_bytes);
                        total_sig2.set(total_bytes);
                    }
                    ConnectProgress::Log(line) => {
                        let mut lines = logs_sig2();
                        lines.push(line);
                        if lines.len() > 200 {
                            let drop = lines.len() - 200;
                            lines.drain(0..drop);
                        }
                        logs_sig2.set(lines);
                    }
                    ConnectProgress::GameLaunched { exe_path: _ } => {
                        if game_launched_at_sig2().is_none() {
                            let launched_at = Instant::now();
                            game_launched_at_sig2.set(Some(launched_at));

                            let mut show_connect_modal_sig3 = show_connect_modal_sig2;
                            let connecting_sig3 = connecting_sig2;
                            let connect_success_sig3 = connect_success_sig2;
                            let game_launched_at_sig3 = game_launched_at_sig2;
                            let last_activity_sig3 = last_activity_sig2;
                            spawn(async move {
                                tokio::time::sleep(Duration::from_secs(10)).await;

                                if !show_connect_modal_sig3() {
                                    return;
                                }

                                // Only close if connection finished successfully,
                                // and the user didn't interact with the launcher after the game started.
                                if !connecting_sig3()
                                    && connect_success_sig3()
                                    && game_launched_at_sig3() == Some(launched_at)
                                    && last_activity_sig3() <= launched_at
                                {
                                    show_connect_modal_sig3.set(false);
                                }
                            });
                        }
                    }
                }
            }
        });

        let res = tokio::task::spawn_blocking(move || {
            crate::connect::connect_to_ss14_address(
                &address,
                account,
                Some(tx),
                Some(cancel_flag),
            )
        })
        .await;

        match res {
            Ok(Ok(ok)) => {
                connect_success_sig.set(ok.launched);
                msg_sig.set(Some(ok.message));
            }
            Ok(Err(e)) => msg_sig.set(Some(format!("ошибка подключения: {e}"))),
            Err(e) => msg_sig.set(Some(format!("ошибка задачи: {e}"))),
        }

        connecting_sig.set(false);
        cancel_sig.set(None);
    });
}

fn format_bytes(bytes: u64) -> String {
    const KB: f64 = 1024.0;
    const MB: f64 = 1024.0 * 1024.0;
    const GB: f64 = 1024.0 * 1024.0 * 1024.0;

    let b = bytes as f64;
    if b >= GB {
        format!("{:.1} GiB", b / GB)
    } else if b >= MB {
        format!("{:.1} MiB", b / MB)
    } else if b >= KB {
        format!("{:.1} KiB", b / KB)
    } else {
        format!("{} B", bytes)
    }
}
