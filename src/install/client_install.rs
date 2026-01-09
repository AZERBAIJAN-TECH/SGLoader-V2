use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

use crate::cancel_flag::CancelFlag;
use crate::connect_progress::{self, ProgressTx};

pub struct ClientInstall {
    pub engine_zip: PathBuf,
    pub engine_signature_hex: String,
}

pub fn ensure_client_installed(
    data_dir: &Path,
    engine_version: &str,
    progress: Option<&ProgressTx>,
    cancel: Option<&CancelFlag>,
) -> Result<ClientInstall, String> {
    let engines_dir = data_dir.join("engines");
    let build = crate::robust_builds::resolve_engine_build(engine_version)?;
    connect_progress::log(
        progress,
        format!(
            "engine_version={} resolved={}",
            engine_version, build.resolved_version
        ),
    );
    let engine_dir = engines_dir.join(sanitize_dir_component(&build.resolved_version));
    let zip_path = engine_dir.join("engine.zip");

    fs::create_dir_all(&engine_dir).map_err(|e| format!("создание каталога движка: {e}"))?;

    let needs_download = !zip_path.exists();
    if needs_download {
        if let Some(c) = cancel {
            c.check()?;
        }
        download_to_file(&build.url, &zip_path, progress, cancel)?;
    }

    // Verify engine sha256 from robust manifest.
    let actual = sha256_file_hex(&zip_path)?;
    if !eq_hex_case_insensitive(&actual, &build.sha256) {
        // Redownload once.
        let _ = fs::remove_file(&zip_path);
        if let Some(c) = cancel {
            c.check()?;
        }
        download_to_file(&build.url, &zip_path, progress, cancel)?;
        let actual2 = sha256_file_hex(&zip_path)?;
        if !eq_hex_case_insensitive(&actual2, &build.sha256) {
            return Err("хеш engine.zip не совпадает (sha256)".to_string());
        }
    }
    Ok(ClientInstall {
        engine_zip: zip_path,
        engine_signature_hex: build.signature,
    })
}

fn download_to_file(
    url: &str,
    path: &Path,
    progress: Option<&ProgressTx>,
    cancel: Option<&CancelFlag>,
) -> Result<(), String> {
    let client = crate::launcher_mask::blocking_http_client_download()?;

    let mut resp = crate::http_config::blocking_send_idempotent_with_retry(|| {
        client
            .get(url)
            .header(reqwest::header::ACCEPT_ENCODING, "identity")
    })
    .map_err(|e| format!("скачивание {url}: {e}"))?;

    if !resp.status().is_success() {
        return Err(format!("скачивание {url}: status {}", resp.status()));
    }

    let total = resp.content_length();
    connect_progress::log(progress, format!("скачивание движка: {url}"));

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
            connect_progress::download(progress, "движок", done, total);
        }

        file.write_all(&buf[..read])
            .map_err(|e| format!("запись файла {:?}: {e}", path))?;
    }

    connect_progress::download(progress, "движок", done, total);

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

fn eq_hex_case_insensitive(a: &str, b: &str) -> bool {
    a.trim().eq_ignore_ascii_case(b.trim())
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
