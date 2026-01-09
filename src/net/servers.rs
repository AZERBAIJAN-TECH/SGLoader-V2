use reqwest::{Client, StatusCode};
use serde::Deserialize;

use crate::storage::hub_urls;
use crate::{ss14_server_info::ServerInfo, ss14_uri};

#[derive(Clone, Debug)]
pub struct ServerEntry {
    pub address: String,
    pub name: String,
    pub players: u32,
    pub max_players: u32,
    pub tags: Vec<String>,
    pub region: Option<String>,
    pub ping_ms: Option<u32>,
    pub online: bool,
    pub description: Option<String>,
}

pub async fn fetch_server_list() -> Result<Vec<ServerEntry>, String> {
    let hub_urls = hub_urls::load_hub_urls();

    let client = crate::launcher_mask::async_http_client()?;
    let mut errors: Vec<String> = Vec::new();

    for base in hub_urls.iter() {
        match fetch_from_hub(&client, base.as_str()).await {
            Ok(entries) => {
                let mapped = entries
                    .into_iter()
                    .map(HubServerListEntry::into_server_entry)
                    .collect();
                return Ok(mapped);
            }
            Err(err) => errors.push(err),
        }
    }

    Err(errors.join("\n"))
}

pub async fn fetch_server_description(address: &str) -> Result<Option<String>, String> {
    let ss14 = ss14_uri::parse_ss14_uri(address)?;
    let info_url = ss14_uri::server_info_url(&ss14)?;

    let client = crate::launcher_mask::async_http_client()?;
    let response = crate::http_config::async_send_idempotent_with_retry(|| client.get(info_url.as_str()))
        .await
        .map_err(|e| format!("{}: {e}", info_url.as_str()))?;

    let status = response.status();
    let bytes = response
        .bytes()
        .await
        .map_err(|e| format!("{}: read body: {e}", info_url.as_str()))?;

    if !status.is_success() {
        let snippet = String::from_utf8_lossy(&bytes);
        let trimmed = snippet.chars().take(160).collect::<String>();
        return Err(format!(
            "{}: status {} body: {}",
            info_url.as_str(),
            status,
            trimmed
        ));
    }

    let info: ServerInfo = serde_json::from_slice(&bytes).map_err(|e| {
        let snippet = String::from_utf8_lossy(&bytes);
        let trimmed = snippet.chars().take(160).collect::<String>();
        format!("{}: parse error {e} body: {trimmed}", info_url.as_str())
    })?;

    Ok(info
        .desc
        .and_then(|d| {
            let trimmed = d.trim();
            (!trimmed.is_empty()).then(|| trimmed.to_string())
        }))
}

async fn fetch_from_hub(client: &Client, base: &str) -> Result<Vec<HubServerListEntry>, String> {
    let url = format!("{base}api/servers");
    let response = crate::http_config::async_send_idempotent_with_retry(|| client.get(&url))
        .await
        .map_err(|e| format!("{url}: {e}"))?;
    let status = response.status();

    if status == StatusCode::NOT_FOUND {
        return Err(format!("{url}: 404"));
    }

    if !status.is_success() {
        let snippet = response
            .text()
            .await
            .unwrap_or_else(|_| "<no body>".to_string());
        let trimmed = snippet.chars().take(160).collect::<String>();
        return Err(format!("{url}: status {} body: {}", status, trimmed));
    }

    let bytes = response
        .bytes()
        .await
        .map_err(|e| format!("{url}: read body: {e}"))?;
    serde_json::from_slice::<Vec<HubServerListEntry>>(&bytes).map_err(|e| {
        let snippet = String::from_utf8_lossy(&bytes);
        let trimmed = snippet.chars().take(160).collect::<String>();
        format!("{url}: parse error {e} body: {trimmed}")
    })
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct HubServerListEntry {
    address: String,
    #[serde(rename = "statusData", alias = "status_data")]
    status_data: HubServerStatus,
}

#[derive(Debug, Deserialize)]
struct HubServerStatus {
    name: Option<String>,
    players: i32,
    #[serde(default)]
    soft_max_players: i32,
    #[serde(default)]
    tags: Vec<String>,
    #[allow(dead_code)]
    #[serde(default)]
    round_start_time: Option<String>,
    #[allow(dead_code)]
    #[serde(default)]
    run_level: Option<i32>,
    #[serde(default)]
    description: Option<String>,
}

impl HubServerListEntry {
    fn into_server_entry(self) -> ServerEntry {
        let HubServerListEntry {
            address,
            status_data,
        } = self;
        let HubServerStatus {
            name,
            players,
            soft_max_players,
            tags,
            description,
            ..
        } = status_data;

        let players = players.max(0) as u32;
        let soft_max_players = soft_max_players.max(0) as u32;

        let region = tags
            .iter()
            .find_map(|t| t.strip_prefix("region:").map(|s| s.to_string()));

        ServerEntry {
            address: address.clone(),
            name: name.unwrap_or_else(|| address.clone()),
            players,
            max_players: if soft_max_players == 0 {
                players.max(1)
            } else {
                soft_max_players
            },
            tags,
            region,
            ping_ms: None,
            online: true,
            description,
        }
    }
}
