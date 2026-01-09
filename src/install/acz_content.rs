use std::collections::{HashMap, HashSet, VecDeque};
use std::fs;
use std::io::{BufWriter, Read, Seek, SeekFrom, Write};
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use blake2::Blake2bVar;
use blake2::digest::{Update, VariableOutput};
use reqwest::header::{ACCEPT_ENCODING, CONTENT_ENCODING, CONTENT_TYPE};

use crate::cancel_flag::CancelFlag;
use crate::connect_progress::{self, ProgressTx};
use crate::ss14_server_info::ServerBuildInformation;

const MANIFEST_DOWNLOAD_PROTOCOL_VERSION: i32 = 1;
const DEFAULT_ACZ_DOWNLOAD_CONCURRENCY: usize = 8;
const ZIP_COPY_BUF_SIZE: usize = 256 * 1024;
const ZIP_DEDUP_READ_MAX: u64 = 4 * 1024 * 1024;

#[derive(Debug, Clone)]
struct ManifestEntry {
    path: String,
    hash: [u8; 32],
}

pub fn build_overlay_zip_from_manifest(
    data_dir: &Path,
    build: &ServerBuildInformation,
    out_zip: &Path,
    progress: Option<&ProgressTx>,
    cancel: Option<&CancelFlag>,
) -> Result<(), String> {
    let manifest_url = build
        .manifest_url
        .as_deref()
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .ok_or_else(|| "acz=true, но build.manifest_url отсутствует".to_string())?;

    let download_url = build
        .manifest_download_url
        .as_deref()
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .ok_or_else(|| "acz=true, но build.manifest_download_url отсутствует".to_string())?;

    let expected_manifest_hash = build
        .manifest_hash
        .as_deref()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());

    let client = crate::launcher_mask::blocking_http_client_download()?;

    // Fetch manifest.
    let progress_tx = progress.cloned();
    let global_done = Arc::new(AtomicU64::new(0));
    let reporter_stop = Arc::new(AtomicBool::new(false));
    let mut reporter: Option<std::thread::JoinHandle<()>> = None;
    if let Some(c) = cancel {
        c.check()?;
    }
    connect_progress::stage(progress, "скачиваем manifest");
    let resp = crate::http_config::blocking_send_idempotent_with_retry(|| {
        client
            .get(manifest_url)
            // Prefer zstd if supported by server (as official launcher does).
            .header(ACCEPT_ENCODING, "zstd")
    })
    .map_err(|e| format!("скачивание manifest {manifest_url}: {e}"))?;

    if !resp.status().is_success() {
        return Err(format!(
            "скачивание manifest {manifest_url}: status {}",
            resp.status()
        ));
    }

    let manifest_bytes = read_response_bytes_maybe_zstd(resp, "manifest", progress)?;

    let (entries, actual_hash) = parse_manifest_and_hash(&manifest_bytes)?;
    if let Some(expected) = expected_manifest_hash
        && !actual_hash.eq_ignore_ascii_case(&expected)
    {
        return Err(format!(
            "manifest_hash не совпадает: expected={expected} actual={actual_hash}"
        ));
    }

    if let Some(c) = cancel {
        c.check()?;
    }

    // Build dedupe map: hash -> paths.
    let mut paths_by_hash: HashMap<[u8; 32], Vec<String>> = HashMap::new();
    for e in &entries {
        paths_by_hash
            .entry(e.hash)
            .or_default()
            .push(e.path.clone());
    }

    // First occurrence per hash (manifest indices are what the /download endpoint expects).
    let mut seen: HashSet<[u8; 32]> = HashSet::new();
    let mut unique: Vec<(i32, [u8; 32])> = Vec::new();
    for (idx, e) in entries.iter().enumerate() {
        if seen.insert(e.hash) {
            unique.push((idx as i32, e.hash));
        }
    }

    // Blob cache: persisted across servers/builds by hash.
    let cache_root_path = data_dir.join("content_blob_cache").join("blake2b-256");
    fs::create_dir_all(&cache_root_path)
        .map_err(|e| format!("создание каталога blob cache: {e}"))?;

    let mut indices_to_download: Vec<i32> = Vec::new();
    for (idx, hash) in &unique {
        let cache_path = blob_cache_path(&cache_root_path, hash);
        if !cache_path.exists() {
            indices_to_download.push(*idx);
        }
    }

    if !indices_to_download.is_empty() {
        // OPTIONS to check protocol.
        {
            connect_progress::stage(progress, "проверяем протокол download");
            let resp = crate::http_config::blocking_send_idempotent_with_retry(|| {
                client.request(reqwest::Method::OPTIONS, download_url)
            })
            .map_err(|e| format!("OPTIONS {download_url}: {e}"))?;
            if !resp.status().is_success() {
                return Err(format!("OPTIONS {download_url}: status {}", resp.status()));
            }

            let min = resp
                .headers()
                .get("X-Robust-Download-Min-Protocol")
                .and_then(|v| v.to_str().ok())
                .and_then(|s| s.parse::<i32>().ok())
                .ok_or_else(|| "download server: нет X-Robust-Download-Min-Protocol".to_string())?;

            let max = resp
                .headers()
                .get("X-Robust-Download-Max-Protocol")
                .and_then(|v| v.to_str().ok())
                .and_then(|s| s.parse::<i32>().ok())
                .ok_or_else(|| "download server: нет X-Robust-Download-Max-Protocol".to_string())?;

            if min > MANIFEST_DOWNLOAD_PROTOCOL_VERSION || max < MANIFEST_DOWNLOAD_PROTOCOL_VERSION
            {
                return Err(format!(
                    "download server protocol not supported: min={min} max={max}"
                ));
            }
        }

        connect_progress::stage(progress, "скачиваем недостающие blobs");

        let download_url = download_url.to_string();
        let entries = std::sync::Arc::new(entries);
        let cache_root = std::sync::Arc::new(cache_root_path.clone());
        let cancel = cancel.cloned();
        let progress: Option<ProgressTx> = None;

        // Aggregated progress reporter (single thread) to avoid multi-thread sender contention.
        if let Some(tx) = progress_tx.clone() {
            let stop = reporter_stop.clone();
            let done = global_done.clone();
            reporter = Some(std::thread::spawn(move || {
                let mut last: u64 = 0;
                while !stop.load(Ordering::Relaxed) {
                    let cur = done.load(Ordering::Relaxed);
                    if cur != last {
                        last = cur;
                        connect_progress::download(Some(&tx), "blobs", cur, None);
                    }
                    std::thread::sleep(std::time::Duration::from_millis(200));
                }
                let cur = done.load(Ordering::Relaxed);
                connect_progress::download(Some(&tx), "blobs", cur, None);
            }));
        }

        let requested_concurrency = std::env::var("SGLOADER_ACZ_DOWNLOAD_CONCURRENCY")
            .ok()
            .and_then(|v| v.trim().parse::<usize>().ok())
            .filter(|v| *v > 0)
            .unwrap_or(DEFAULT_ACZ_DOWNLOAD_CONCURRENCY)
            .min(indices_to_download.len().max(1))
            .max(1);

        let batch_size = std::env::var("SGLOADER_ACZ_DOWNLOAD_BATCH_SIZE")
            .ok()
            .and_then(|v| v.trim().parse::<usize>().ok())
            .filter(|v| *v > 0)
            .unwrap_or_else(|| {
                // Keep request overhead reasonable: aim for ~4 requests per worker.
                // This helps reduce the long-tail without making everything slower.
                let target_batches = requested_concurrency.saturating_mul(4).max(1);
                let computed = indices_to_download.len().div_ceil(target_batches);
                computed.clamp(64, 4096)
            });

        let batches: VecDeque<Vec<i32>> = indices_to_download
            .chunks(batch_size)
            .map(|chunk| chunk.to_vec())
            .collect();

        let num_batches = batches.len().max(1);
        let concurrency = requested_concurrency.min(num_batches);

        let queue = Arc::new(Mutex::new(batches));
        let abort = Arc::new(AtomicBool::new(false));
        let mut handles = Vec::new();

        for _ in 0..concurrency {
            let download_url = download_url.clone();
            let entries = entries.clone();
            let cache_root = cache_root.clone();
            let cancel = cancel.clone();
            let progress = progress.clone();
            let global_done = global_done.clone();
            let queue = queue.clone();
            let abort = abort.clone();

            let handle = std::thread::spawn(move || {
                let client = crate::launcher_mask::blocking_http_client_download()?;
                loop {
                    if abort.load(Ordering::Relaxed) {
                        return Ok(());
                    }

                    let batch = {
                        let mut q = queue
                            .lock()
                            .map_err(|_| "mutex queue poisoned in blob downloader".to_string())?;
                        q.pop_front()
                    };

                    let Some(batch) = batch else {
                        return Ok(());
                    };

                    if let Err(e) = download_blob_chunk_into_cache(
                        &client,
                        &download_url,
                        &entries,
                        &cache_root,
                        &batch,
                        progress.as_ref(),
                        Some(global_done.as_ref()),
                        cancel.as_ref(),
                    ) {
                        abort.store(true, Ordering::Relaxed);
                        return Err(e);
                    }
                }
            });

            handles.push(handle);
        }

        for h in handles {
            match h.join() {
                Ok(Ok(())) => {}
                Ok(Err(e)) => return Err(e),
                Err(_) => return Err("panic в потоке скачивания blobs".to_string()),
            }
        }
    } else {
        connect_progress::stage(progress, "blobs уже в кэше");
    }

    reporter_stop.store(true, Ordering::Relaxed);
    if let Some(r) = reporter {
        let _ = r.join();
    }

    // Prepare zip writer.
    if let Some(parent) = out_zip.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("mkdir {:?}: {e}", parent))?;
    }

    let file = fs::File::create(out_zip).map_err(|e| format!("create {:?}: {e}", out_zip))?;
    let file = BufWriter::new(file);
    let mut zip = zip::ZipWriter::new(file);

    connect_progress::stage(progress, "собираем overlay zip");

    for (_idx, hash) in unique {
        if let Some(c) = cancel {
            c.check()?;
        }
        let cache_path = blob_cache_path(&cache_root_path, &hash);
        if !cache_path.exists() {
            return Err(format!("не найден blob в кэше: {}", cache_path.display()));
        }

        let mut f =
            fs::File::open(&cache_path).map_err(|e| format!("open {:?}: {e}", cache_path))?;
        let Some(paths) = paths_by_hash.get(&hash) else {
            continue;
        };

        // If multiple manifest paths map to the same blob, avoid rereading from disk for small blobs.
        if paths.len() > 1
            && let Ok(meta) = fs::metadata(&cache_path)
            && meta.len() <= ZIP_DEDUP_READ_MAX
        {
            let mut data = Vec::with_capacity(meta.len() as usize);
            f.read_to_end(&mut data)
                .map_err(|e| format!("read {:?}: {e}", cache_path))?;
            for p in paths {
                let name = p.replace('\\', "/");
                let opts: zip::write::FileOptions<'_, ()> = zip::write::FileOptions::default()
                    .compression_method(zip::CompressionMethod::Stored);
                zip.start_file(name, opts)
                    .map_err(|e| format!("zip start_file: {e}"))?;
                zip.write_all(&data)
                    .map_err(|e| format!("zip write: {e}"))?;
            }
            continue;
        }

        let mut copy_buf: Vec<u8> = vec![0u8; ZIP_COPY_BUF_SIZE];

        for p in paths {
            f.seek(SeekFrom::Start(0))
                .map_err(|e| format!("seek {:?}: {e}", cache_path))?;

            let name = p.replace('\\', "/");
            let opts: zip::write::FileOptions<'_, ()> = zip::write::FileOptions::default()
                .compression_method(zip::CompressionMethod::Stored);
            zip.start_file(name, opts)
                .map_err(|e| format!("zip start_file: {e}"))?;
            copy_with_buffer(&mut f, &mut zip, copy_buf.as_mut_slice())
                .map_err(|e| format!("zip write: {e}"))?;
        }
    }

    zip.finish()
        .map_err(|e| format!("finalize zip {:?}: {e}", out_zip))?;

    Ok(())
}

fn read_response_bytes_maybe_zstd(
    resp: reqwest::blocking::Response,
    label: &str,
    progress: Option<&ProgressTx>,
) -> Result<Vec<u8>, String> {
    let is_zstd = resp
        .headers()
        .get(CONTENT_ENCODING)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.split(',').any(|p| p.trim().eq_ignore_ascii_case("zstd")))
        .unwrap_or(false);

    let total = if is_zstd { None } else { resp.content_length() };

    let mut bytes = Vec::new();
    if is_zstd {
        let mut decoder =
            zstd::stream::read::Decoder::new(resp).map_err(|e| format!("zstd decoder: {e}"))?;
        read_to_end_with_progress(&mut decoder, &mut bytes, label, progress, total)?;
    } else {
        let mut r = resp;
        read_to_end_with_progress(&mut r, &mut bytes, label, progress, total)?;
    }

    Ok(bytes)
}

fn read_to_end_with_progress(
    reader: &mut dyn Read,
    out: &mut Vec<u8>,
    label: &str,
    progress: Option<&ProgressTx>,
    total: Option<u64>,
) -> Result<(), String> {
    let mut buf = [0u8; 1024 * 64];
    let mut done: u64 = 0;
    let mut last_emit: u64 = 0;
    const EMIT_EVERY: u64 = 2 * 1024 * 1024;

    loop {
        let read = reader
            .read(&mut buf)
            .map_err(|e| format!("read response: {e}"))?;
        if read == 0 {
            break;
        }

        out.extend_from_slice(&buf[..read]);
        done += read as u64;
        if done.saturating_sub(last_emit) >= EMIT_EVERY {
            last_emit = done;
            connect_progress::download(progress, label, done, total);
        }
    }

    connect_progress::download(progress, label, done, total);
    Ok(())
}

fn parse_manifest_and_hash(bytes: &[u8]) -> Result<(Vec<ManifestEntry>, String), String> {
    // Hash the raw manifest bytes as the official launcher does (BLAKE2b-256, no key).
    let mut hasher = Blake2bVar::new(32).map_err(|e| format!("blake2 init: {e}"))?;
    hasher.update(bytes);
    let mut out = [0u8; 32];
    hasher
        .finalize_variable(&mut out)
        .map_err(|e| format!("blake2 finalize: {e}"))?;

    let mut entries = Vec::new();
    let text = String::from_utf8_lossy(bytes);
    let mut lines = text.lines();
    let header = lines.next().unwrap_or("");
    if header.trim() != "Robust Content Manifest 1" {
        return Err("неизвестный заголовок manifest".to_string());
    }

    for line in lines {
        let line = line.trim_end();
        if line.is_empty() {
            continue;
        }
        let Some(sep) = line.find(' ') else {
            return Err("битая строка manifest".to_string());
        };
        let hash_hex = &line[..sep];
        let path = line[sep + 1..].to_string();
        let hash_vec = hex::decode(hash_hex).map_err(|_| "битый hash в manifest".to_string())?;
        if hash_vec.len() != 32 {
            return Err("hash в manifest не 32 байта".to_string());
        }
        let mut hash = [0u8; 32];
        hash.copy_from_slice(&hash_vec);
        entries.push(ManifestEntry { path, hash });
    }

    Ok((entries, hex::encode_upper(out)))
}

fn blob_cache_path(cache_root: &Path, hash: &[u8; 32]) -> std::path::PathBuf {
    // Small fanout to avoid too many files per directory.
    let prefix = format!("{:02x}{:02x}", hash[0], hash[1]);
    cache_root
        .join(prefix)
        .join(format!("{}.blob", hex::encode(hash)))
}

fn temp_cache_path(final_path: &Path) -> std::path::PathBuf {
    let suffix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let name = final_path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("blob.tmp");
    final_path.with_file_name(format!("{name}.tmp.{suffix}"))
}

fn download_blob_chunk_into_cache(
    client: &reqwest::blocking::Client,
    download_url: &str,
    entries: &std::sync::Arc<Vec<ManifestEntry>>,
    cache_root: &std::sync::Arc<std::path::PathBuf>,
    indices: &[i32],
    progress: Option<&ProgressTx>,
    global_done: Option<&AtomicU64>,
    cancel: Option<&CancelFlag>,
) -> Result<(), String> {
    // POST request body: little-endian i32 indices.
    let mut body = Vec::with_capacity(indices.len() * 4);
    for idx in indices {
        body.extend_from_slice(&idx.to_le_bytes());
    }

    let req = client
        .post(download_url)
        .header(
            "X-Robust-Download-Protocol",
            MANIFEST_DOWNLOAD_PROTOCOL_VERSION.to_string(),
        )
        .header(ACCEPT_ENCODING, "zstd")
        .header(CONTENT_TYPE, "application/octet-stream")
        .body(body);

    let resp = req
        .send()
        .map_err(|e| format!("скачивание content blobs {download_url}: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!(
            "скачивание content blobs {download_url}: status {}",
            resp.status()
        ));
    }

    let is_zstd = resp
        .headers()
        .get(CONTENT_ENCODING)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.split(',').any(|p| p.trim().eq_ignore_ascii_case("zstd")))
        .unwrap_or(false);
    let total = if is_zstd { None } else { resp.content_length() };

    let reader: Box<dyn Read> = if is_zstd {
        Box::new(zstd::stream::read::Decoder::new(resp).map_err(|e| format!("zstd decoder: {e}"))?)
    } else {
        Box::new(resp)
    };

    let mut reader = ProgressRead::new(reader, progress, "blobs", total, global_done);
    let flags = read_i32_le_reader(&mut reader)?;
    let precompressed = (flags & 1) != 0;

    for idx in indices {
        if let Some(c) = cancel {
            c.check()?;
        }

        let entry = &entries[*idx as usize];
        let uncompressed_len = read_i32_le_reader(&mut reader)? as usize;

        let cache_path = blob_cache_path(cache_root.as_path(), &entry.hash);
        if cache_path.exists() {
            // Another concurrent run may have populated it; still must consume bytes from stream.
            if precompressed {
                let compressed_len = read_i32_le_reader(&mut reader)? as i32;
                if compressed_len > 0 {
                    discard_exact_reader(&mut reader, compressed_len as usize, cancel)?;
                } else {
                    discard_exact_reader(&mut reader, uncompressed_len, cancel)?;
                }
            } else {
                discard_exact_reader(&mut reader, uncompressed_len, cancel)?;
            }
            continue;
        }

        let temp_path = temp_cache_path(&cache_path);
        if let Some(parent) = temp_path.parent() {
            fs::create_dir_all(parent).map_err(|e| format!("mkdir {:?}: {e}", parent))?;
        }
        let file =
            fs::File::create(&temp_path).map_err(|e| format!("create {:?}: {e}", temp_path))?;
        let mut file = BufWriter::new(file);

        let mut hasher = Blake2bVar::new(32).map_err(|e| format!("blake2 init: {e}"))?;

        let written = if precompressed {
            let compressed_len = read_i32_le_reader(&mut reader)? as i32;
            if compressed_len > 0 {
                let clen = compressed_len as u64;
                let mut limited = (&mut reader).take(clen);
                let mut decoder = zstd::stream::read::Decoder::new(&mut limited)
                    .map_err(|e| format!("zstd decoder: {e}"))?;
                let written = copy_read_exact_len_with_hash(
                    &mut decoder,
                    &mut file,
                    uncompressed_len,
                    &mut hasher,
                    cancel,
                )?;
                let _ = std::io::copy(&mut decoder, &mut std::io::sink());
                let _ = std::io::copy(&mut limited, &mut std::io::sink());
                written
            } else {
                copy_read_exact_len_with_hash(
                    &mut reader,
                    &mut file,
                    uncompressed_len,
                    &mut hasher,
                    cancel,
                )?
            }
        } else {
            copy_read_exact_len_with_hash(
                &mut reader,
                &mut file,
                uncompressed_len,
                &mut hasher,
                cancel,
            )?
        };

        if written != uncompressed_len {
            let _ = fs::remove_file(&temp_path);
            return Err("неверный размер распаковки blob".to_string());
        }

        let mut out = [0u8; 32];
        hasher
            .finalize_variable(&mut out)
            .map_err(|e| format!("blake2 finalize: {e}"))?;
        if out != entry.hash {
            let _ = fs::remove_file(&temp_path);
            return Err("hash mismatch while downloading content".to_string());
        }

        file.flush().map_err(|e| format!("flush cache: {e}"))?;
        drop(file);
        match fs::rename(&temp_path, &cache_path) {
            Ok(()) => {}
            Err(_) => {
                if cache_path.exists() {
                    let _ = fs::remove_file(&temp_path);
                } else {
                    fs::copy(&temp_path, &cache_path)
                        .map_err(|e| format!("cache copy {:?}: {e}", cache_path))?;
                    let _ = fs::remove_file(&temp_path);
                }
            }
        }
    }

    Ok(())
}

fn copy_with_buffer(
    reader: &mut dyn Read,
    writer: &mut dyn Write,
    buf: &mut [u8],
) -> std::io::Result<u64> {
    let mut total: u64 = 0;
    loop {
        let n = reader.read(buf)?;
        if n == 0 {
            break;
        }
        writer.write_all(&buf[..n])?;
        total += n as u64;
    }
    Ok(total)
}

fn copy_read_exact_len_with_hash(
    reader: &mut dyn Read,
    writer: &mut dyn Write,
    len: usize,
    hasher: &mut Blake2bVar,
    cancel: Option<&CancelFlag>,
) -> Result<usize, String> {
    let mut buf = [0u8; 1024 * 64];
    let mut done: usize = 0;

    while done < len {
        if let Some(c) = cancel
            && c.is_cancelled()
        {
            return Err("отменено".to_string());
        }

        let to_read = (len - done).min(buf.len());
        let n = reader
            .read(&mut buf[..to_read])
            .map_err(|e| format!("read payload: {e}"))?;
        if n == 0 {
            return Err("короткий ответ download stream (payload)".to_string());
        }

        hasher.update(&buf[..n]);
        writer
            .write_all(&buf[..n])
            .map_err(|e| format!("write cache: {e}"))?;
        done += n;
    }

    Ok(done)
}

fn discard_exact_reader(
    reader: &mut dyn Read,
    len: usize,
    cancel: Option<&CancelFlag>,
) -> Result<(), String> {
    let mut buf = [0u8; 1024 * 64];
    let mut done: usize = 0;
    while done < len {
        if let Some(c) = cancel
            && c.is_cancelled()
        {
            return Err("отменено".to_string());
        }
        let to_read = (len - done).min(buf.len());
        let n = reader
            .read(&mut buf[..to_read])
            .map_err(|e| format!("read payload: {e}"))?;
        if n == 0 {
            return Err("короткий ответ download stream (payload)".to_string());
        }
        done += n;
    }
    Ok(())
}

fn read_i32_le_reader(reader: &mut dyn Read) -> Result<i32, String> {
    let mut b = [0u8; 4];
    reader
        .read_exact(&mut b)
        .map_err(|_| "короткий ответ download stream".to_string())?;
    Ok(i32::from_le_bytes(b))
}

struct ProgressRead<'a> {
    inner: Box<dyn Read>,
    progress: Option<&'a ProgressTx>,
    global_done: Option<&'a AtomicU64>,
    label: String,
    total: Option<u64>,
    done: u64,
    last_emit: u64,
}

impl<'a> ProgressRead<'a> {
    fn new(
        inner: Box<dyn Read>,
        progress: Option<&'a ProgressTx>,
        label: &str,
        total: Option<u64>,
        global_done: Option<&'a AtomicU64>,
    ) -> Self {
        Self {
            inner,
            progress,
            global_done,
            label: label.to_string(),
            total,
            done: 0,
            last_emit: 0,
        }
    }

    fn emit(&mut self) {
        const EMIT_EVERY: u64 = 2 * 1024 * 1024;
        if self.done.saturating_sub(self.last_emit) < EMIT_EVERY {
            return;
        }
        self.last_emit = self.done;
        if let Some(tx) = self.progress {
            connect_progress::download(Some(tx), &self.label, self.done, self.total);
        }
    }
}

impl Read for ProgressRead<'_> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let n = self.inner.read(buf)?;
        if n > 0 {
            self.done += n as u64;
            if let Some(g) = self.global_done {
                g.fetch_add(n as u64, Ordering::Relaxed);
            }
            self.emit();
        } else if let Some(tx) = self.progress {
            connect_progress::download(Some(tx), &self.label, self.done, self.total);
        }
        Ok(n)
    }
}
