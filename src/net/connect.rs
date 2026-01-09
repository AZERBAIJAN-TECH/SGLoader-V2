use std::io::{Read, Seek, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::process::Stdio;
use std::{fs, io};

#[cfg(all(target_os = "windows", not(debug_assertions)))]
use std::os::windows::process::CommandExt;

use url::Url;

use crate::auth::LoginInfo;
use crate::cancel_flag::CancelFlag;
use crate::connect_progress::{self, ProgressTx};
use crate::ss14_server_info::{AuthMode, ServerInfo};
use crate::ss14_uri;

const AUTH_SERVER_PRIMARY: &str = "https://auth.spacestation14.com/";

pub struct ConnectResult {
    pub launched: bool,
    pub message: String,
}

pub fn connect_to_ss14_address(
    address: &str,
    account: Option<LoginInfo>,
    progress: Option<ProgressTx>,
    cancel: Option<CancelFlag>,
) -> Result<ConnectResult, String> {
    if let Some(c) = &cancel {
        c.check()?;
    }
    connect_progress::stage(progress.as_ref(), "получаем /info");
    connect_progress::log(progress.as_ref(), format!("address={address}"));

    let ss14 = ss14_uri::parse_ss14_uri(address)?;
    let info_url = ss14_uri::server_info_url(&ss14)?;

    let http = crate::launcher_mask::blocking_http_client_api()?;

    let info_resp =
        crate::http_config::blocking_send_idempotent_with_retry(|| http.get(info_url.as_str()))
            .map_err(|e| format!("info запрос: {e}"))?;
    let info: ServerInfo = info_resp
        .error_for_status()
        .map_err(|e| format!("info статус: {e}"))?
        .json()
        .map_err(|e| format!("info parse: {e}"))?;

    let connect_addr = get_connect_address(&info, &info_url)?;
    connect_progress::log(progress.as_ref(), format!("connect_address={connect_addr}"));

    if let Some(c) = &cancel {
        c.check()?;
    }

    let mut build = info
        .build_information
        .clone()
        .ok_or_else(|| "сервер не вернул build информацию".to_string())?;

    // Prefer build-provided URLs.
    // Only infer self-hosted fallbacks if the server didn't provide them.
    let download_url_missing = build
        .download_url
        .as_deref()
        .map(|s| s.trim().is_empty())
        .unwrap_or(true);
    if download_url_missing {
        build.download_url = Some(ss14_uri::server_selfhosted_client_zip_url(&ss14)?.to_string());
    }

    // Some servers set ACZ-related URLs even when acz=false, and some CDNs protect the zip download.
    // Keep parity with SS14.Launcher fallbacks by inferring these URLs when missing.
    {
        let api_base = ss14_uri::server_api_base(&ss14)?;

        let manifest_url_missing = build
            .manifest_url
            .as_deref()
            .map(|s| s.trim().is_empty())
            .unwrap_or(true);
        if manifest_url_missing {
            build.manifest_url = Some(
                api_base
                    .join("manifest.txt")
                    .map_err(|e| e.to_string())?
                    .to_string(),
            );
        }

        let manifest_download_url_missing = build
            .manifest_download_url
            .as_deref()
            .map(|s| s.trim().is_empty())
            .unwrap_or(true);
        if manifest_download_url_missing {
            build.manifest_download_url = Some(
                api_base
                    .join("download")
                    .map_err(|e| e.to_string())?
                    .to_string(),
            );
        }
    }

    if info.auth_information.mode == AuthMode::Required && account.is_none() {
        return Err("сервер требует авторизацию — войдите в аккаунт".to_string());
    }

    let data_dir = crate::app_paths::data_dir()?;

    // Content is required to start the client (Content.* assemblies/resources).
    // We pass it to SS14.Loader via SS14_LOADER_OVERLAY_ZIP.
    // Some servers return a CDN URL that may be protected; fall back to server-hosted /client.zip.
    connect_progress::stage(progress.as_ref(), "проверяем/скачиваем контент");
    let fallback_zip_url = ss14_uri::server_selfhosted_client_zip_url(&ss14)
        .ok()
        .map(|u| u.to_string());
    let overlay_zip = crate::content_install::ensure_content_overlay_zip(
        &data_dir,
        &build,
        fallback_zip_url.as_deref(),
        progress.as_ref(),
        cancel.as_ref(),
    )?;

    connect_progress::log(
        progress.as_ref(),
        format!("content_overlay_zip={}", overlay_zip.display()),
    );

    // IMPORTANT: build.download_url / manifest_url относятся к контенту.
    // Движок (Robust.Client) скачивается через robust-builds manifest, как в SS14.Launcher.
    connect_progress::stage(progress.as_ref(), "проверяем/скачиваем движок");
    let install = crate::client_install::ensure_client_installed(
        &data_dir,
        &build.engine_version,
        progress.as_ref(),
        cancel.as_ref(),
    )?;

    connect_progress::log(
        progress.as_ref(),
        format!("engine_zip={}", install.engine_zip.display()),
    );

    let mut args: Vec<String> = Vec::new();

    let username = account
        .as_ref()
        .map(|a| a.username.clone())
        .unwrap_or_else(|| "Player".to_string());

    args.push("--username".to_string());
    args.push(username);

    // Minimal set of CVars used by the official launcher.
    args.push("--cvar".to_string());
    args.push("display.compat=false".to_string());

    args.push("--cvar".to_string());
    args.push("launch.launcher=true".to_string());

    args.push("--launcher".to_string());
    args.push("--connect-address".to_string());
    args.push(connect_addr);

    args.push("--ss14-address".to_string());
    args.push(ss14.to_string());

    // build.* CVars (important for modern CDN / content plumbing).
    push_build_cvar(&mut args, "download_url", build.download_url.as_deref());
    push_build_cvar(&mut args, "manifest_url", build.manifest_url.as_deref());
    push_build_cvar(
        &mut args,
        "manifest_download_url",
        build.manifest_download_url.as_deref(),
    );
    push_build_cvar(&mut args, "version", Some(build.version.as_str()));
    push_build_cvar(&mut args, "fork_id", Some(build.fork_id.as_str()));
    push_build_cvar(&mut args, "hash", build.hash.as_deref());
    push_build_cvar(&mut args, "manifest_hash", build.manifest_hash.as_deref());
    push_build_cvar(
        &mut args,
        "engine_version",
        Some(build.engine_version.as_str()),
    );

    let mut env: Vec<(String, String)> = Vec::new();
    if info.auth_information.mode != AuthMode::Disabled
        && let Some(acc) = &account
    {
        env.push(("ROBUST_AUTH_TOKEN".to_string(), acc.token.token.clone()));
        env.push(("ROBUST_AUTH_USERID".to_string(), acc.user_id.to_string()));
        env.push((
            "ROBUST_AUTH_PUBKEY".to_string(),
            info.auth_information.public_key.clone(),
        ));
        env.push((
            "ROBUST_AUTH_SERVER".to_string(),
            AUTH_SERVER_PRIMARY.to_string(),
        ));
    }

    env.push((
        "SS14_LOADER_OVERLAY_ZIP".to_string(),
        overlay_zip.to_string_lossy().to_string(),
    ));

    connect_progress::stage(progress.as_ref(), "запускаем клиент");

    if let Some(c) = &cancel {
        c.check()?;
    }

    let cfg = crate::settings::load_settings().unwrap_or_default();
    let security = cfg.security.clone();

    // Launcher integration (Redial): only advertise launcher if not disabled.
    if !security.disable_redial
        && let Ok(exe) = std::env::current_exe()
    {
        env.push((
            "SS14_LAUNCHER_PATH".to_string(),
            exe.to_string_lossy().to_string(),
        ));
    }

    if security.autodelete_hwid {
        connect_progress::log(
            progress.as_ref(),
            "autodelete hwid: очищаем HKCU\\Software\\Space Wizards\\Robust",
        );
        if let Err(e) = crate::core::hwid_cleanup::clear_robust_hkcu_values() {
            connect_progress::log(progress.as_ref(), format!("autodelete hwid: ошибка: {e}"));
        }
    }

    let marsey_ctx = crate::marsey::MarseyLaunchContext {
        engine_version: build.engine_version.clone(),
        fork_id: build.fork_id.clone(),
        hide_level: security.hide_level.to_marsey_value().to_string(),
        disable_redial: security.disable_redial,
    };
    let launched = launch_client(
        &install,
        &args,
        &env,
        &marsey_ctx,
        progress.as_ref(),
    )?;

    Ok(ConnectResult {
        launched: true,
        message: format!("запущено: {}", launched.display()),
    })
}

fn push_build_cvar(args: &mut Vec<String>, name: &str, value: Option<&str>) {
    let Some(v) = value else {
        return;
    };
    if v.is_empty() {
        return;
    }
    args.push("--cvar".to_string());
    args.push(format!("build.{name}={v}"));
}

fn get_connect_address(info: &ServerInfo, info_url: &Url) -> Result<String, String> {
    if let Some(addr) = &info.connect_address {
        let trimmed = addr.trim();
        if !trimmed.is_empty() {
            // /info commonly returns host:port (without scheme). Robust expects a URL-like address.
            if trimmed.contains("://") {
                if let Ok(parsed) = Url::parse(trimmed) {
                    return Ok(parsed.to_string());
                }
            } else if let Ok(parsed) = Url::parse(&format!("udp://{trimmed}")) {
                return Ok(parsed.to_string());
            }
            // If the server provided a connect_address but it's malformed, fall back to the host/port we used for /info.
        }
    }

    let host = info_url
        .host_str()
        .ok_or_else(|| "не удалось определить host".to_string())?;

    let port = info_url.port().unwrap_or(1212);

    let parsed = Url::parse(&format!("udp://{host}:{port}"))
        .map_err(|_| "не удалось собрать udp адрес".to_string())?;
    Ok(parsed.to_string())
}

fn launch_client(
    install: &crate::client_install::ClientInstall,
    args: &[String],
    env: &[(String, String)],
    marsey: &crate::marsey::MarseyLaunchContext,
    progress: Option<&ProgressTx>,
) -> Result<PathBuf, String> {
    let data_dir = crate::app_paths::data_dir()?;
    let loader = crate::ss14_loader::ensure_loader_installed(&data_dir)?;

    // Prelaunch: verify engine signature in Rust (so the managed loader can stay thin).
    // The managed loader can skip verification when this succeeds.
    match crate::ss14::engine_signature::verify_engine_signature(
        &install.engine_zip,
        &install.engine_signature_hex,
        &loader.public_key,
    ) {
        Ok(()) => {}
        Err(e) => {
            if crate::ss14::engine_signature::should_allow_disable_signing_on_debug() {
                connect_progress::log(
                    progress,
                    format!(
                        "[SGLOADER] engine signature не прошла проверку, но SS14_DISABLE_SIGNING включён (debug): {e}"
                    ),
                );
            } else {
                return Err(e);
            }
        }
    }

    // Rust-side Redial server: keep it alive globally and pass its pipe name to the loader.
    let redial_pipe_name = std::env::current_exe()
        .ok()
        .and_then(|exe| {
            crate::net::redial_pipe::ensure_global_redial_pipe(marsey.disable_redial, &exe).ok()
        })
        .flatten();

    let mut marsey_batch = if loader.marsey_enabled {
        Some(
            crate::marsey::prepare_pipes_for_launch(&data_dir, marsey)
                .map_err(|e| format!("Marsey prepare: {e}"))?,
        )
    } else {
        None
    };

    let log_path = make_launch_log_path(&data_dir)?;
    // Auto-mitigation for a known Marsey backports crash (Version.CompareTo called with a string).
    // We keep backports enabled by default, but if SS14.Loader exits immediately with this signature,
    // retry once with backports disabled via MarseyConf.
    let mut auto_disabled_backports = false;
    let mut first_attempt_tail: Option<String> = None;

    for attempt in 0..2 {
        let log_file = fs::OpenOptions::new()
            .create(true)
            .truncate(true)
            .write(true)
            .open(&log_path)
            .map_err(|e| format!("не удалось создать лог запуска {:?}: {e}", log_path))?;
        let log_file_err = log_file
            .try_clone()
            .map_err(|e| format!("не удалось открыть stderr лог: {e}"))?;

        if auto_disabled_backports {
            let _ = writeln!(
                &log_file_err,
                "[SGLOADER] Авто-фикс: отключаем Marsey backports из-за крэша сравнения Version; повторный запуск."
            );
        }

        // Optional diagnostics for Marsey IPC. Enable with `SGLOADER_MARSEY_DIAGNOSTICS=1`.
        let marsey_diag_enabled = std::env::var("SGLOADER_MARSEY_DIAGNOSTICS")
            .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
            .unwrap_or(false);
        if marsey_diag_enabled && let Some(batch) = &marsey_batch {
            let marsey_count = if batch.marsey.trim().is_empty() {
                0
            } else {
                batch.marsey.split(',').count()
            };
            let subverter_count = if batch.subverter.trim().is_empty() {
                0
            } else {
                batch.subverter.split(',').count()
            };
            let preload_count = if batch.preload.trim().is_empty() {
                0
            } else {
                batch.preload.split(',').count()
            };

            let _ = writeln!(
                &log_file_err,
                "[SGLOADER] Marsey IPC prepared: preload={preload_count} marsey={marsey_count} subverter={subverter_count}"
            );
        }

        let mut cmd = if loader
            .entrypoint
            .extension()
            .and_then(|s| s.to_str())
            .map(|s| s.eq_ignore_ascii_case("dll"))
            .unwrap_or(false)
        {
            let mut c = Command::new("dotnet");
            c.arg(&loader.entrypoint);
            c
        } else {
            Command::new(&loader.entrypoint)
        };

        // SS14.Loader args: <robustPath> <signatureHex> <publicKeyPath> [engineArg...]
        cmd.arg(&install.engine_zip);
        cmd.arg(&install.engine_signature_hex);
        cmd.arg(&loader.public_key);
        cmd.args(args);

        for (k, v) in env {
            cmd.env(k, v);
        }

        // We already verified in Rust; allow managed loader to skip.
        cmd.env("SS14_LOADER_SKIP_SIGNATURE_VERIFY", "1");

        if let Some(name) = &redial_pipe_name {
            cmd.env("SGLOADER_REDIAL_PIPE", name);
        }

        cmd.stdout(Stdio::from(log_file));
        cmd.stderr(Stdio::from(log_file_err));

        // Windows native DLL resolution depends on cwd and PATH.
        // - SS14.Loader's own native deps should resolve from the loader directory.
        // - Robust engine native deps (e.g. SDL3.dll) are expected next to / extracted alongside the engine zip.
        // If we set cwd to the loader directory, engine-native DLLs may not be found.
        let loader_dir = loader
            .entrypoint
            .parent()
            .ok_or_else(|| "не удалось определить каталог SS14.Loader".to_string())?;

        let engine_dir = install
            .engine_zip
            .parent()
            .ok_or_else(|| "не удалось определить каталог engine.zip".to_string())?;

        // Keep cwd as the loader directory. Some Robust content/resource logic relies on the
        // process working directory; switching it to the engine dir can break resource mounting.
        // Native DLL discovery is handled via PATH below.
        cmd.current_dir(loader_dir);

        // In release builds, don't flash a console window for SS14.Loader.
        #[cfg(all(target_os = "windows", not(debug_assertions)))]
        {
            const CREATE_NO_WINDOW: u32 = 0x0800_0000;
            cmd.creation_flags(CREATE_NO_WINDOW);
        }

        // Also prepend both dirs to PATH so both sets of native deps are discoverable regardless of cwd.
        // (On Windows, PATH is ';' separated; on Unix it's ':'.)
        let path_key = if cfg!(target_os = "windows") { "Path" } else { "PATH" };
        let sep = if cfg!(target_os = "windows") { ";" } else { ":" };
        let existing_path = std::env::var_os(path_key).unwrap_or_default();
        let mut new_path = std::ffi::OsString::new();
        new_path.push(loader_dir.as_os_str());
        new_path.push(sep);
        new_path.push(engine_dir.as_os_str());
        if !existing_path.is_empty() {
            new_path.push(sep);
            new_path.push(existing_path);
        }
        cmd.env(path_key, new_path);

        // Spawn pipe senders shortly before launching the loader.
        // Only for Marsey-enabled loader builds.
        let pipe_thread = marsey_batch
            .clone()
            .map(|batch| std::thread::spawn(move || crate::marsey::send_pipes(batch)));

        let mut child = cmd
            .spawn()
            .map_err(|e| format!("не удалось запустить SS14.Loader: {e}"))?;

        // Countdown for auto-close in UI must start only after the process is actually spawned.
        connect_progress::game_launched(
            progress,
            loader.entrypoint.to_string_lossy().to_string(),
        );

        // If MarseyConf IPC fails, patches will crash the rewrite loader; fail early.
        if let Some(t) = pipe_thread
            && let Err(e) = t
                .join()
                .unwrap_or_else(|_| Err("Marsey IPC thread panic".to_string()))
        {
            let _ = child.kill();
            return Err(format!("Marsey IPC error: {e}"));
        }

        // If the process dies immediately (black screen then close), surface the log.
        std::thread::sleep(std::time::Duration::from_millis(800));
        if let Some(status) = child
            .try_wait()
            .map_err(|e| format!("не удалось проверить статус SS14.Loader: {e}"))?
        {
            let tail = read_log_tail(&log_path, 16 * 1024).unwrap_or_else(|_| String::new());

            if attempt == 0
                && loader.marsey_enabled
                && !auto_disabled_backports
                && marsey_batch.is_some()
                && is_marsey_backports_version_compare_crash(&tail)
            {
                first_attempt_tail = Some(tail);
                auto_disabled_backports = true;
                marsey_batch = marsey_batch.as_ref().map(|b| {
                    let mut nb = b.clone();
                    nb.marsey_conf =
                        crate::marsey::with_marsey_backports_enabled(&nb.marsey_conf, false);
                    nb
                });
                continue;
            }

            let mut msg = format!(
                "SS14.Loader завершился сразу (code={}). Лог: {}",
                status.code().unwrap_or(-1),
                log_path.display()
            );

            if auto_disabled_backports {
                msg.push_str("\n\n[SGLOADER] Пробовали авто-выключение Marsey backports из-за крэша Version.CompareTo.");
            }

            if let Some(t0) = &first_attempt_tail
                && !t0.trim().is_empty()
            {
                msg.push_str("\n\n--- попытка 1 (до авто-фикса) ---\n");
                msg.push_str(t0.trim());
            }

            if !tail.trim().is_empty() {
                msg.push_str("\n\n--- попытка 2 ---\n");
                msg.push_str(tail.trim());
            }

            return Err(msg);
        }

        return Ok(loader.entrypoint);
    }

    Err("SS14.Loader завершился сразу (неизвестная ошибка)".to_string())
}

fn make_launch_log_path(data_dir: &Path) -> Result<PathBuf, String> {
    let logs = data_dir.join("logs");
    fs::create_dir_all(&logs).map_err(|e| format!("mkdir {:?}: {e}", logs))?;
    Ok(logs.join("last-launch.log"))
}

fn read_log_tail(path: &Path, max_bytes: u64) -> io::Result<String> {
    let mut file = fs::File::open(path)?;
    let len = file.metadata()?.len();
    let start = len.saturating_sub(max_bytes);
    if start > 0 {
        file.seek(io::SeekFrom::Start(start))?;
    }

    let mut buf = Vec::new();
    file.read_to_end(&mut buf)?;
    Ok(String::from_utf8_lossy(&buf).to_string())
}

fn is_marsey_backports_version_compare_crash(log_text: &str) -> bool {
    let lc = log_text.to_ascii_lowercase();
    lc.contains("object must be of type version")
        && (lc.contains("marseyportman") || lc.contains("validatebackport"))
}
