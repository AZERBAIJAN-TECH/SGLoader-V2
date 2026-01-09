use std::fs;
use std::path::PathBuf;
use std::sync::OnceLock;

use reqwest::header::{HeaderMap, HeaderValue, USER_AGENT};

static FINGERPRINT: OnceLock<String> = OnceLock::new();

const FINGERPRINT_FILE_NAME: &str = "fingerprint.txt";

pub fn fingerprint() -> Result<String, String> {
    if let Some(v) = FINGERPRINT.get() {
        return Ok(v.clone());
    }

    let fp = load_or_create_fingerprint()?;
    let _ = FINGERPRINT.set(fp.clone());
    Ok(fp)
}

pub fn user_agent_value() -> String {
    // We intentionally mimic the official launcher name.
    // Version doesn't need to match exactly; some CDNs only require the product token.
    // Match the official launcher version token to reduce CDN variance.
    "SS14.Launcher/59".to_string()
}

pub fn default_headers(fingerprint: &str) -> Result<HeaderMap, String> {
    let mut headers = HeaderMap::new();
    let ua = HeaderValue::from_str(&user_agent_value())
        .map_err(|_| "не удалось собрать User-Agent".to_string())?;
    headers.insert(USER_AGENT, ua);

    let fp = HeaderValue::from_str(fingerprint)
        .map_err(|_| "не удалось собрать SS14-Launcher-Fingerprint".to_string())?;
    headers.insert("SS14-Launcher-Fingerprint", fp);
    Ok(headers)
}

pub fn blocking_http_client() -> Result<reqwest::blocking::Client, String> {
    let fp = fingerprint()?;
    let headers = default_headers(&fp)?;
    crate::http_config::build_blocking_client_with_headers(
        headers,
        crate::http_config::HttpProfile::Download,
    )
}

pub fn blocking_http_client_api() -> Result<reqwest::blocking::Client, String> {
    let fp = fingerprint()?;
    let headers = default_headers(&fp)?;
    crate::http_config::build_blocking_client_with_headers(
        headers,
        crate::http_config::HttpProfile::Api,
    )
}

pub fn blocking_http_client_download() -> Result<reqwest::blocking::Client, String> {
    blocking_http_client()
}

pub fn async_http_client() -> Result<reqwest::Client, String> {
    let fp = fingerprint()?;
    let headers = default_headers(&fp)?;
    crate::http_config::build_async_client_with_headers(
        headers,
        crate::http_config::HttpProfile::Api,
    )
}

fn load_or_create_fingerprint() -> Result<String, String> {
    let path = fingerprint_path()?;
    if let Ok(existing) = fs::read_to_string(&path) {
        let s = existing.trim().to_string();
        if is_uuid_like(&s) {
            return Ok(s);
        }
    }

    let fp = uuid::Uuid::new_v4().to_string();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("mkdir {:?}: {e}", parent))?;
    }
    fs::write(&path, fp.as_bytes()).map_err(|e| format!("write {:?}: {e}", path))?;
    Ok(fp)
}

fn fingerprint_path() -> Result<PathBuf, String> {
    Ok(crate::app_paths::data_dir()?.join(FINGERPRINT_FILE_NAME))
}

fn is_uuid_like(s: &str) -> bool {
    // Accept canonical UUID string. Don't hard-parse to keep file corruption non-fatal.
    let s = s.trim();
    if s.len() != 36 {
        return false;
    }
    let mut dash_positions = 0;
    for (i, c) in s.chars().enumerate() {
        match i {
            8 | 13 | 18 | 23 => {
                if c != '-' {
                    return false;
                }
                dash_positions += 1;
            }
            _ => {
                if !c.is_ascii_hexdigit() {
                    return false;
                }
            }
        }
    }
    dash_positions == 4
}
