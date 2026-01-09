use dioxus::prelude::*;

use crate::net::news;

fn format_time(ts: chrono::DateTime<chrono::Utc>) -> String {
    // Simple, locale-neutral formatting.
    ts.format("%Y-%m-%d %H:%M UTC").to_string()
}

async fn load_posts() -> Result<Vec<news::NewsPost>, String> {
    news::fetch_news(50).await
}

#[component]
pub fn tab_news() -> Element {
    let posts: Signal<Vec<news::NewsPost>> = use_signal(Vec::new);
    let mut loading = use_signal(|| true);
    let mut error: Signal<Option<String>> = use_signal(|| None);
    let mut open_post_id: Signal<Option<String>> = use_signal(|| None);

    {
        let mut posts = posts;
        let mut loading = loading;
        let mut error = error;
        use_future(move || async move {
            loading.set(true);
            match load_posts().await {
                Ok(list) => {
                    posts.set(list);
                    error.set(None);
                }
                Err(e) => error.set(Some(e)),
            }
            loading.set(false);
        });
    }

    rsx! {
        div { class: "news-page",
            button {
                class: "ghost news-refresh",
                disabled: loading(),
                onclick: move |_| {
                    if loading() {
                        return;
                    }
                    loading.set(true);
                    error.set(None);
                    let mut posts2 = posts;
                    let mut loading2 = loading;
                    let mut error2 = error;
                    spawn(async move {
                        match load_posts().await {
                            Ok(list) => {
                                posts2.set(list);
                                error2.set(None);
                            }
                            Err(e) => error2.set(Some(e)),
                        }
                        loading2.set(false);
                    });
                },
                "Обновить"
            }

            if loading() {
                p { class: "status status-info", "Загрузка новостей..." }
            }

            if let Some(msg) = error() {
                p { class: "status status-error selectable", {msg} }
            }

            if !loading() && error().is_none() {
                if posts().is_empty() {
                    p { class: "status status-info", "Новостей пока нет." }
                }

                for post in posts().into_iter() {
                    div { class: "section news-post",
                        div { class: "news-post-header",
                            div { class: "news-post-meta",
                                h2 { class: "news-title", {post.title} }
                                p { class: "news-date", {format_time(post.created_at)} }
                            }
                            button {
                                class: "ghost news-open",
                                onclick: {
                                    let post_id = post.id.clone();
                                    move |_| {
                                        let is_open = open_post_id().as_deref() == Some(post_id.as_str());
                                        if is_open {
                                            open_post_id.set(None);
                                        } else {
                                            open_post_id.set(Some(post_id.clone()));
                                        }
                                    }
                                },
                                if open_post_id().as_deref() == Some(post.id.as_str()) {
                                    "Скрыть"
                                } else {
                                    "Открыть"
                                }
                            }
                        }

                        if open_post_id().as_deref() == Some(post.id.as_str()) {
                            for block in post.blocks.into_iter() {
                                match block {
                                    news::NewsBlock::Text { text } => rsx!(
                                        p { class: "news-text selectable", {text} }
                                    ),
                                    news::NewsBlock::Image { media_id, alt } => {
                                        if news::is_safe_media_id(&media_id) {
                                            let src = news::media_url(&media_id);
                                            rsx!(
                                                img { class: "news-image", src: "{src}", alt: "{alt}" }
                                            )
                                        } else {
                                            rsx!(Fragment {})
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
}
