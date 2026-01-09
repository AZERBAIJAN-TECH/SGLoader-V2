use std::collections::HashSet;
use std::fs;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

const HUB_URLS_FILE_NAME: &str = "hub_urls.json";

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct HubUrlsFile {
    urls: Vec<String>,
}

pub fn default_hub_urls() -> Vec<String> {
    vec![
        "https://hub.spacestation14.com/".to_string(),
        "https://hub.fallback.spacestation14.com/".to_string(),
    ]
}

pub fn load_hub_urls() -> Vec<String> {
    match try_load_hub_urls() {
        Ok(urls) if !urls.is_empty() => urls,
        _ => default_hub_urls(),
    }
}

pub fn try_load_hub_urls() -> Result<Vec<String>, String> {
    let path = hub_urls_file_path()?;
    let contents = match fs::read_to_string(&path) {
        Ok(data) => data,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(default_hub_urls()),
        Err(err) => return Err(format!("не удалось прочитать ссылки хаба: {err}")),
    };

    let stored: HubUrlsFile = serde_json::from_str(&contents)
        .map_err(|err| format!("не удалось разобрать ссылки хаба: {err}"))?;

    normalize_and_validate_urls(&stored.urls)
}

pub fn save_hub_urls(urls: &[String]) -> Result<Vec<String>, String> {
    let dir = crate::app_paths::data_dir()?;
    fs::create_dir_all(&dir)
        .map_err(|err| format!("не удалось создать каталог для настроек хаба: {err}"))?;

    let normalized = normalize_and_validate_urls(urls)?;
    let path = hub_urls_file_path()?;

    let stored = HubUrlsFile {
        urls: normalized.clone(),
    };
    let json = serde_json::to_string_pretty(&stored)
        .map_err(|err| format!("не удалось сериализовать ссылки хаба: {err}"))?;

    fs::write(&path, json).map_err(|err| format!("не удалось записать ссылки хаба: {err}"))?;

    Ok(normalized)
}

fn hub_urls_file_path() -> Result<PathBuf, String> {
    Ok(crate::app_paths::data_dir()?.join(HUB_URLS_FILE_NAME))
}

fn normalize_and_validate_urls(raw: &[String]) -> Result<Vec<String>, String> {
    let mut seen: HashSet<String> = HashSet::new();
    let mut out: Vec<String> = Vec::new();

    for s in raw {
        let mut url = s.trim().to_string();
        if url.is_empty() {
            continue;
        }

        if !(url.starts_with("https://") || url.starts_with("http://")) {
            return Err(format!(
                "некорректная ссылка хаба: {url} (нужен http/https)"
            ));
        }

        if !url.ends_with('/') {
            url.push('/');
        }

        if seen.insert(url.clone()) {
            out.push(url);
        }
    }

    if out.is_empty() {
        return Err("список ссылок хаба пуст".to_string());
    }

    Ok(out)
}
