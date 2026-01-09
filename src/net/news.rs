use chrono::{DateTime, Utc};
use reqwest::Client;
use serde::Deserialize;

use crate::constants::NEWS_API_BASE_URL;
use crate::http_config::{self, HttpProfile};

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type")]
pub enum NewsBlock {
    #[serde(rename = "text")]
    Text { text: String },
    #[serde(rename = "image")]
    Image { media_id: String, #[serde(default)] alt: String },
}

#[derive(Debug, Clone, Deserialize)]
pub struct NewsPost {
    pub id: String,
    pub title: String,
    pub created_at: DateTime<Utc>,
    #[serde(default)]
    pub blocks: Vec<NewsBlock>,
}

#[derive(Debug, Clone, Deserialize)]
struct NewsListResponse {
    posts: Vec<NewsPost>,
}

fn base_url() -> String {
    NEWS_API_BASE_URL.trim_end_matches('/').to_string()
}

pub fn is_safe_media_id(media_id: &str) -> bool {
    // UUIDs and similar identifiers only.
    let s = media_id.trim();
    !s.is_empty()
        && s.len() <= 80
        && s.bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'-')
}

pub fn media_url(media_id: &str) -> String {
    format!("{}/api/news/media/{}", base_url(), media_id)
}

pub async fn fetch_news(limit: usize) -> Result<Vec<NewsPost>, String> {
    let limit = limit.clamp(1, 200);

    let client: Client = http_config::build_async_client(HttpProfile::Api)
        .unwrap_or_else(|_| Client::new());

    let url = format!("{}/api/news?limit={}", base_url(), limit);

    let resp = http_config::async_send_idempotent_with_retry(|| client.get(&url))
        .await
        .map_err(|e| format!("news request: {e}"))?;

    if !resp.status().is_success() {
        return Err(format!("news status: {}", resp.status()));
    }

    let mut parsed: NewsListResponse = resp
        .json()
        .await
        .map_err(|e| format!("news parse: {e}"))?;

    parsed.posts.sort_by(|a, b| b.created_at.cmp(&a.created_at));
    Ok(parsed.posts)
}
