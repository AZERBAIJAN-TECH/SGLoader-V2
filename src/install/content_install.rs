use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

use crate::cancel_flag::CancelFlag;
use crate::connect_progress::{self, ProgressTx};
use crate::ss14_server_info::ServerBuildInformation;

pub fn ensure_content_overlay_zip(
    data_dir: &Path,
    build: &ServerBuildInformation,
    fallback_download_url: Option<&str>,
    progress: Option<&ProgressTx>,
    cancel: Option<&CancelFlag>,
) -> Result<PathBuf, String> {
    let primary_url = build
        .download_url
        .as_deref()
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .ok_or_else(|| "сервер не вернул build.download_url".to_string())?;

    let key = if let Some(h) = build
        .hash
        .as_deref()
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
    {
        h
    } else if let Some(h) = build
        .manifest_hash
        .as_deref()
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
    {
        h
    } else {
        build.version.as_str()
    };

    let content_dir = data_dir.join("content").join(sanitize_dir_component(key));
    let zip_path = content_dir.join("client.zip");
    let acz_marker = content_dir.join("client.zip.acz_overlay");

    // Preferred overlay cache: keyed by manifest_hash (content identity), not by build.hash (zip bytes).
    let overlay_cache_zip: Option<PathBuf> = build
        .manifest_hash
        .as_deref()
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .map(|h| {
            data_dir
                .join("content_overlay_cache")
                .join(sanitize_dir_component(h))
                .join("client.zip")
        });
    let overlay_cache_marker: Option<PathBuf> = overlay_cache_zip
        .as_ref()
        .and_then(|p| p.parent().map(|d| d.join("client.zip.acz_overlay")));

    fs::create_dir_all(&content_dir).map_err(|e| format!("создание каталога контента: {e}"))?;

    // If we already have a cached overlay zip for this manifest, prefer it.
    if let (Some(overlay_zip), Some(marker)) = (&overlay_cache_zip, &overlay_cache_marker)
        && overlay_zip.exists()
        && marker.exists()
    {
        return Ok(overlay_zip.clone());
    }

    let mut needs_download = !zip_path.exists();

    // If the overlay zip was produced via the manifest pipeline, build.hash is not applicable.
    // Preserve the file and skip sha256 validation, otherwise we'd rebuild on every launch.
    if !needs_download {
        if acz_marker.exists() {
            return Ok(zip_path);
        }
    } else if acz_marker.exists() {
        let _ = fs::remove_file(&acz_marker);
    }

    if !needs_download
        && let Some(expected) = build
            .hash
            .as_deref()
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
    {
        let actual = sha256_file_hex(&zip_path)?;
        if !actual.eq_ignore_ascii_case(expected) {
            let _ = fs::remove_file(&zip_path);
            needs_download = true;
        }
    }

    if needs_download {
        if let Some(c) = cancel {
            c.check()?;
        }

        let downloaded_zip: bool;

        // Default path: download the content zip.
        connect_progress::log(progress, format!("content key={key}"));
        match download_to_file_with_fallback(
            primary_url,
            fallback_download_url,
            &zip_path,
            progress,
            cancel,
        ) {
            Ok(()) => {
                downloaded_zip = true;
            }
            Err(zip_err) => {
                // If CDN zip is protected (401/403), try ACZ manifest pipeline as a fallback.
                let can_try_manifest = build
                    .manifest_url
                    .as_deref()
                    .map(|s| !s.trim().is_empty())
                    .unwrap_or(false)
                    && build
                        .manifest_download_url
                        .as_deref()
                        .map(|s| !s.trim().is_empty())
                        .unwrap_or(false);

                let looks_like_auth = zip_err.contains("status 401")
                    || zip_err.contains("status 403")
                    || zip_err.contains("Unauthorized")
                    || zip_err.contains("Forbidden");

                if can_try_manifest && looks_like_auth {
                    let _ = fs::remove_file(&zip_path);
                    if let Some(c) = cancel {
                        c.check()?;
                    }
                    connect_progress::stage(progress, "скачиваем контент через manifest");

                    // If we have a manifest_hash, build into the overlay cache location.
                    let out_zip = overlay_cache_zip.as_deref().unwrap_or(&zip_path);
                    if let Some(parent) = out_zip.parent() {
                        let _ = fs::create_dir_all(parent);
                    }
                    match crate::acz_content::build_overlay_zip_from_manifest(
                        data_dir, build, out_zip, progress, cancel,
                    ) {
                        Ok(()) => {}
                        Err(acz_err) => {
                            return Err(format!(
                                "скачивание контента не удалось (zip): {zip_err}\nи acz/manifest тоже не удалось: {acz_err}"
                            ));
                        }
                    }

                    // Mark this zip as an overlay produced via manifest download.
                    // It is not expected to match build.hash (sha256 of SS14.Client.zip).
                    if out_zip == zip_path {
                        let _ = fs::write(&acz_marker, b"acz\n");
                    }
                    if let Some(marker) = overlay_cache_marker.as_deref() {
                        let _ = fs::write(marker, b"acz\n");
                    }

                    // ACZ builds an overlay zip; build.hash is for the official SS14.Client.zip bytes and won't match.
                    downloaded_zip = false;

                    if let Some(overlay_zip) = overlay_cache_zip
                        && overlay_zip.exists()
                    {
                        return Ok(overlay_zip);
                    }
                } else {
                    return Err(zip_err);
                }
            }
        }

        if downloaded_zip
            && let Some(expected) = build
                .hash
                .as_deref()
                .map(|s| s.trim())
                .filter(|s| !s.is_empty())
        {
            let actual = sha256_file_hex(&zip_path)?;
            if !actual.eq_ignore_ascii_case(expected) {
                let _ = fs::remove_file(&zip_path);
                return Err("хеш client.zip не совпадает (sha256)".to_string());
            }
        }
    }

    Ok(zip_path)
}

fn download_to_file_with_fallback(
    primary_url: &str,
    fallback_url: Option<&str>,
    path: &Path,
    progress: Option<&ProgressTx>,
    cancel: Option<&CancelFlag>,
) -> Result<(), String> {
    match download_to_file(primary_url, path, "контент", progress, cancel) {
        Ok(()) => Ok(()),
        Err(e) => {
            let Some(fallback) = fallback_url.map(|s| s.trim()).filter(|s| !s.is_empty()) else {
                return Err(e);
            };
            if fallback.eq_ignore_ascii_case(primary_url) {
                return Err(e);
            }

            // Common CDN protection responses. If we get one of these, try the server-hosted client.zip.
            let should_try_fallback = e.contains("status 401")
                || e.contains("status 403")
                || e.contains("status 404")
                || e.contains("Unauthorized")
                || e.contains("Forbidden")
                || e.contains("Not Found");

            if !should_try_fallback {
                return Err(e);
            }

            // Remove partial file if any.
            let _ = fs::remove_file(path);
            download_to_file(fallback, path, "контент (fallback)", progress, cancel).map_err(|e2| {
                format!(
                    "скачивание контента не удалось. primary={primary_url} err={e}\nfallback={fallback} err={e2}"
                )
            })
        }
    }
}

fn download_to_file(
    url: &str,
    path: &Path,
    label: &str,
    progress: Option<&ProgressTx>,
    cancel: Option<&CancelFlag>,
) -> Result<(), String> {
    let client = crate::launcher_mask::blocking_http_client_download()?;

    let mut resp = crate::http_config::blocking_send_idempotent_with_retry(|| {
        client
            .get(url)
            // IMPORTANT: We must save the exact bytes (sha256 must match server-provided hash).
            // reqwest can transparently decompress gzip/deflate/br if the server sets Content-Encoding,
            // so request identity for ZIP downloads.
            .header(reqwest::header::ACCEPT_ENCODING, "identity")
    })
    .map_err(|e| format!("скачивание {url}: {e}"))?;

    if !resp.status().is_success() {
        // Try to surface useful diagnostics (WWW-Authenticate, body snippet, etc.).
        let status = resp.status();
        let www_auth: String = resp
            .headers()
            .get("www-authenticate")
            .and_then(|v| v.to_str().ok())
            .map(|s| s.to_string())
            .unwrap_or_default();
        let server: String = resp
            .headers()
            .get("server")
            .and_then(|v| v.to_str().ok())
            .map(|s| s.to_string())
            .unwrap_or_default();

        let mut snippet = String::new();
        if let Ok(bytes) = resp.bytes() {
            let b = bytes;
            let take = b.len().min(512);
            snippet = String::from_utf8_lossy(&b[..take]).to_string();
        }

        let mut extra = String::new();
        if !www_auth.is_empty() {
            extra.push_str(&format!(" WWW-Authenticate={www_auth}"));
        }
        if !server.is_empty() {
            extra.push_str(&format!(" Server={server}"));
        }
        if !snippet.trim().is_empty() {
            extra.push_str(" Body=");
            extra.push_str(snippet.trim());
        }

        return Err(format!("скачивание {url}: status {status}{extra}"));
    }

    let total = resp.content_length();
    connect_progress::log(progress, format!("скачивание {label}: {url}"));

    let mut file = fs::File::create(path).map_err(|e| format!("создание файла {:?}: {e}", path))?;
    let mut buf = [0u8; 1024 * 64];

    let mut done: u64 = 0;
    let mut last_emit: u64 = 0;
    const EMIT_EVERY: u64 = 256 * 1024;

    loop {
        if let Some(c) = cancel
            && c.is_cancelled()
        {
            let _ = fs::remove_file(path);
            return Err("отменено".to_string());
        }
        let read = resp
            .read(&mut buf)
            .map_err(|e| format!("чтение ответа: {e}"))?;
        if read == 0 {
            break;
        }

        done += read as u64;
        if done.saturating_sub(last_emit) >= EMIT_EVERY {
            last_emit = done;
            connect_progress::download(progress, label, done, total);
        }

        file.write_all(&buf[..read])
            .map_err(|e| format!("запись файла {:?}: {e}", path))?;
    }

    connect_progress::download(progress, label, done, total);

    Ok(())
}

fn sha256_file_hex(path: &Path) -> Result<String, String> {
    let mut file = fs::File::open(path).map_err(|e| format!("open {:?}: {e}", path))?;
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 1024 * 64];
    loop {
        let read = file
            .read(&mut buf)
            .map_err(|e| format!("read {:?}: {e}", path))?;
        if read == 0 {
            break;
        }
        hasher.update(&buf[..read]);
    }
    let digest = hasher.finalize();
    Ok(hex::encode(digest))
}

fn sanitize_dir_component(s: &str) -> String {
    s.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '.' || c == '_' || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect()
}
