use std::collections::HashSet;
use std::ffi::OsStr;
use std::path::{Path, PathBuf};

mod dotnet_metadata;
mod pipes;

const PIPE_MARSEY_CONF: &str = "MarseyConf";
const PIPE_PRELOAD: &str = "PreloadMarseyPatchesPipe";
const PIPE_MARSEY: &str = "MarseyPatchesPipe";
const PIPE_SUBVERTER: &str = "SubverterPatchesPipe";

const MARSEY_DIR: &str = "Marsey";
const PATCHES_DIR: &str = "patches";
const LEGACY_MODS_DIR: &str = "Mods";
const RPACKS_DIR: &str = "ResourcePacks";

const PATCHLIST_FILE: &str = "patches.marsey";

#[derive(Debug, Clone)]
pub struct MarseyLaunchContext {
    pub engine_version: String,
    pub fork_id: String,
    pub hide_level: String,
    pub disable_redial: bool,
}

#[derive(Debug, Default)]
struct ScannerOutput {
    preload: Vec<String>,
    marsey: Vec<String>,
    subverter: Vec<String>,
}

fn normalize_case(s: &str) -> String {
    s.to_lowercase()
}

fn normalize_os_case(s: &OsStr) -> String {
    s.to_string_lossy().to_lowercase()
}

fn is_dll_path(p: &Path) -> bool {
    p.extension()
        .map(|s| s.to_string_lossy().eq_ignore_ascii_case("dll"))
        .unwrap_or(false)
}

fn canonicalize_fallback(p: &Path) -> PathBuf {
    std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf())
}

fn escape_percent_and_bytes(s: &str, bytes_to_escape: &[u8]) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.as_bytes() {
        if *b == b'%' || bytes_to_escape.contains(b) {
            out.push('%');
            out.push_str(&format!("{:02X}", b));
        } else {
            out.push(*b as char);
        }
    }
    out
}

fn pipe_encode_token(s: &str) -> String {
    // Delimiter is ',', so escape ',' and '%' only.
    escape_percent_and_bytes(s, b",")
}

fn join_pipe_tokens(items: &[String]) -> String {
    items
        .iter()
        .map(|s| pipe_encode_token(s))
        .collect::<Vec<_>>()
        .join(",")
}

fn conf_encode_value(s: &str) -> String {
    // Conf format uses ';' and '=' as delimiters, so escape ';', '=' and '%'.
    escape_percent_and_bytes(s, b";=")
}

fn list_mod_dlls(mods_dir: &Path) -> Result<Vec<PathBuf>, String> {
    let mut dlls: Vec<PathBuf> = Vec::new();
    if !mods_dir.exists() {
        return Ok(dlls);
    }

    for entry in std::fs::read_dir(mods_dir).map_err(|e| format!("read_dir {:?}: {e}", mods_dir))? {
        let entry = entry.map_err(|e| format!("read_dir {:?}: {e}", mods_dir))?;
        let p = entry.path();
        if !is_dll_path(&p) {
            continue;
        }
        dlls.push(p);
    }

    dlls.sort_by(|a, b| a.file_name().cmp(&b.file_name()));
    Ok(dlls)
}

fn patch_scan_dirs(paths: &MarseyPaths) -> Vec<PathBuf> {
    let mut out = Vec::with_capacity(2);
    out.push(paths.patches_dir.clone());

    // Back-compat: older versions stored patch DLLs under Marsey/Mods.
    if paths.legacy_mods_dir.exists() {
        out.push(paths.legacy_mods_dir.clone());
    }

    out
}

fn list_patch_dlls(mods_dirs: &[PathBuf]) -> Result<Vec<PathBuf>, String> {
    let mut seen_filenames: HashSet<String> = HashSet::new();
    let mut out: Vec<PathBuf> = Vec::new();

    for dir in mods_dirs {
        let dlls = list_mod_dlls(dir)?;
        for p in dlls {
            let Some(name) = p.file_name() else {
                continue;
            };
            let name_norm = normalize_os_case(name);
            if seen_filenames.insert(name_norm) {
                out.push(p);
            }
        }
    }

    out.sort_by(|a, b| a.file_name().cmp(&b.file_name()));
    Ok(out)
}

fn filter_enabled_mod_dlls(dlls: Vec<PathBuf>, enabled: &Option<HashSet<String>>) -> Vec<PathBuf> {
    let Some(enabled) = enabled.as_ref() else {
        return dlls;
    };

    let enabled_norm: HashSet<String> = enabled.iter().map(|s| normalize_case(s)).collect();

    dlls.into_iter()
        .filter(|p| {
            let Some(name) = p.file_name() else {
                return false;
            };
            let name_norm = normalize_os_case(name);
            enabled_norm.contains(&name_norm)
        })
        .collect()
}

pub fn ensure_marsey_dirs(data_dir: &Path) -> Result<MarseyPaths, String> {
    // New preferred location for patch DLLs.
    let patches_dir = data_dir.join(PATCHES_DIR);

    // Marsey still owns its own working directories (e.g. ResourcePacks).
    let marsey_root = data_dir.join(MARSEY_DIR);
    let rpacks_dir = marsey_root.join(RPACKS_DIR);

    // Legacy location that older versions used for patch DLLs.
    // Do NOT auto-create it so new installs don't get the old folder.
    let legacy_mods_dir = marsey_root.join(LEGACY_MODS_DIR);

    std::fs::create_dir_all(&patches_dir).map_err(|e| format!("mkdir {:?}: {e}", patches_dir))?;
    std::fs::create_dir_all(&rpacks_dir).map_err(|e| format!("mkdir {:?}: {e}", rpacks_dir))?;

    Ok(MarseyPaths {
        marsey_root,
        patches_dir,
        legacy_mods_dir,
        patchlist_file: data_dir.join(PATCHLIST_FILE),
    })
}

pub struct MarseyPaths {
    pub marsey_root: PathBuf,
    pub patches_dir: PathBuf,
    pub legacy_mods_dir: PathBuf,
    pub patchlist_file: PathBuf,
}

#[derive(Debug, Clone)]
pub struct PatchEntry {
    pub filename: String,
    pub enabled: bool,
    pub name: String,
    pub description: String,
    pub rdnn: String,
}

pub fn list_patches(data_dir: &Path) -> Result<(PathBuf, Vec<PatchEntry>), String> {
    let paths = ensure_marsey_dirs(data_dir)?;
    let mods_dirs = patch_scan_dirs(&paths);

    let enabled = load_enabled_patch_filenames(&paths)?;
    let enabled_norm: Option<HashSet<String>> = enabled
        .as_ref()
        .map(|set| set.iter().map(|s| normalize_case(s)).collect());

    let mut dlls = list_patch_dlls(&mods_dirs)?;
    dlls.retain(|p| dotnet_metadata::try_classify_patch(p).is_some());

    let mut out: Vec<PatchEntry> = Vec::with_capacity(dlls.len());
    for p in dlls {
        let filename = p
            .file_name()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_default();
        let filename_norm = normalize_case(&filename);
        let enabled = enabled_norm
            .as_ref()
            .map(|set| set.contains(&filename_norm))
            .unwrap_or(true);

        let display = dotnet_metadata::try_read_patch_display_info(&p);

        let name = display
            .as_ref()
            .and_then(|d| d.name.clone())
            .unwrap_or_else(|| filename.trim_end_matches(".dll").to_string());
        let description = display
            .as_ref()
            .and_then(|d| d.description.clone())
            .unwrap_or_default();

        let rdnn = display
            .as_ref()
            .and_then(|d| d.rdnn.clone())
            .or_else(|| try_get_patch_rdnn(&p))
            .unwrap_or_default();

        out.push(PatchEntry {
            filename,
            enabled,
            name,
            description,
            rdnn,
        });
    }

    Ok((paths.patches_dir, out))
}

pub fn set_patch_enabled(data_dir: &Path, filename: &str, enabled: bool) -> Result<(), String> {
    let paths = ensure_marsey_dirs(data_dir)?;
    let mods_dirs = patch_scan_dirs(&paths);

    // Keep patchlist scoped to actual patches only.
    let mut all: Vec<String> = Vec::new();
    let mut dlls = list_patch_dlls(&mods_dirs)?;
    dlls.retain(|p| dotnet_metadata::try_classify_patch(p).is_some());
    for p in dlls {
        let Some(name) = p.file_name() else {
            continue;
        };
        all.push(name.to_string_lossy().to_string());
    }

    let target_norm = normalize_case(filename);

    let mut enabled_actual: HashSet<String> = if paths.patchlist_file.exists() {
        let text = std::fs::read_to_string(&paths.patchlist_file)
            .map_err(|e| format!("read {:?}: {e}", paths.patchlist_file))?;
        let mut set_norm = HashSet::new();
        for line in text.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            set_norm.insert(normalize_case(trimmed));
        }

        all.iter()
            .filter(|n| set_norm.contains(&normalize_case(n)))
            .cloned()
            .collect()
    } else {
        all.iter().cloned().collect()
    };

    if enabled {
        // Re-add with on-disk casing when possible.
        if let Some(actual) = all.iter().find(|n| normalize_case(n) == target_norm) {
            enabled_actual.insert(actual.clone());
        } else {
            enabled_actual.insert(filename.to_string());
        }
    } else {
        enabled_actual.retain(|n| normalize_case(n) != target_norm);
    }

    // If everything is enabled, keep defaults by removing patchlist file.
    let all_norm: HashSet<String> = all.iter().map(|n| normalize_case(n)).collect();
    let enabled_norm: HashSet<String> = enabled_actual.iter().map(|n| normalize_case(n)).collect();
    if enabled_norm == all_norm {
        if paths.patchlist_file.exists() {
            std::fs::remove_file(&paths.patchlist_file)
                .map_err(|e| format!("remove {:?}: {e}", paths.patchlist_file))?;
        }
        return Ok(());
    }

    let mut enabled_sorted: Vec<String> = enabled_actual.into_iter().collect();
    enabled_sorted.sort_by_key(|a| a.to_lowercase());
    let text = enabled_sorted.join("\n");
    std::fs::write(&paths.patchlist_file, text)
        .map_err(|e| format!("write {:?}: {e}", paths.patchlist_file))?;
    Ok(())
}

pub fn try_get_patch_rdnn(path: &Path) -> Option<String> {
    // Most patches use namespace as their reverse-domain identifier.
    dotnet_metadata::try_get_typedef_namespace(path, "MarseyPatch")
        .or_else(|| dotnet_metadata::try_get_typedef_namespace(path, "SubverterPatch"))
}

pub fn prepare_pipes_for_launch(
    data_dir: &Path,
    ctx: &MarseyLaunchContext,
) -> Result<MarseyPipeBatch, String> {
    let paths = ensure_marsey_dirs(data_dir)?;
    let mods_dirs = patch_scan_dirs(&paths);

    let enabled = load_enabled_patch_filenames(&paths)?;
    let mut scan = scan_mods_dir(&mods_dirs, &enabled)?;

    // Always load all enabled DLLs at least once.
    // Some mods rely on module initializers / self-hooking and don't declare MarseyPatch/SubverterPatch.
    let all_enabled = collect_enabled_mod_dlls(&mods_dirs, &enabled)?;

    if !all_enabled.is_empty() {
        let preload_set: HashSet<String> = scan.preload.iter().map(|p| p.to_lowercase()).collect();
        let subverter_set: HashSet<String> =
            scan.subverter.iter().map(|p| p.to_lowercase()).collect();

        let mut marsey_set: HashSet<String> = scan.marsey.iter().map(|p| p.to_string()).collect();

        for p in &all_enabled {
            let norm = p.to_lowercase();
            if preload_set.contains(&norm) || subverter_set.contains(&norm) {
                continue;
            }
            marsey_set.insert(p.to_string());
        }

        let mut merged: Vec<String> = marsey_set.into_iter().collect();
        merged.sort_by_key(|a| a.to_lowercase());
        scan.marsey = merged;
    }

    // If the scanner fails to classify anything, fall back to sending all enabled DLLs via Marsey.
    if scan.preload.is_empty() && scan.marsey.is_empty() && scan.subverter.is_empty() {
        scan.marsey = all_enabled;
    }

    let preload = join_pipe_tokens(&scan.preload);
    let marsey = join_pipe_tokens(&scan.marsey);
    let subverter = join_pipe_tokens(&scan.subverter);

    let marsey_conf = build_marsey_conf_string(ctx);

    Ok(MarseyPipeBatch {
        marsey_conf,
        preload,
        marsey,
        subverter,
    })
}

#[derive(Debug, Clone)]
pub struct MarseyPipeBatch {
    pub marsey_conf: String,
    pub preload: String,
    pub marsey: String,
    pub subverter: String,
}

pub fn with_marsey_backports_enabled(conf: &str, enabled: bool) -> String {
    let v = if enabled { "true" } else { "false" };
    let conf = override_conf_kv(conf, "MARSEY_BACKPORTS", v);

    // Keep existing semantics: "any" backports are allowed unless explicitly disabled.
    // When fully disabling backports, also disable any-backports to avoid ambiguity.
    if enabled {
        override_conf_kv(&conf, "MARSEY_NO_ANY_BACKPORTS", "false")
    } else {
        override_conf_kv(&conf, "MARSEY_NO_ANY_BACKPORTS", "true")
    }
}

fn override_conf_kv(conf: &str, key: &str, value: &str) -> String {
    // Format: key=value;key=value;...
    // Values are expected to not contain ';'.
    let mut out: Vec<(String, String)> = Vec::new();
    let mut replaced = false;

    for seg in conf.split(';') {
        let seg = seg.trim();
        if seg.is_empty() {
            continue;
        }
        let mut it = seg.splitn(2, '=');
        let k = it.next().unwrap_or("").trim();
        let v = it.next().unwrap_or("").trim();
        if k.is_empty() {
            continue;
        }

        if k == key {
            out.push((k.to_string(), value.to_string()));
            replaced = true;
        } else {
            out.push((k.to_string(), v.to_string()));
        }
    }

    if !replaced {
        out.push((key.to_string(), value.to_string()));
    }

    out.into_iter()
        .map(|(k, v)| format!("{k}={v}"))
        .collect::<Vec<_>>()
        .join(";")
}

pub fn send_pipes(batch: MarseyPipeBatch) -> Result<(), String> {
    // Loader may take a while to reach MarseyConf read (zip mount, ALC resolving, etc.).
    let timeout_ms = 60_000u32;

    let conf_data = batch.marsey_conf;
    let preload_data = batch.preload;
    let marsey_data = batch.marsey;
    let subverter_data = batch.subverter;

    let t_conf = std::thread::spawn(move || {
        pipes::send_named_pipe_utf8(PIPE_MARSEY_CONF, &conf_data, timeout_ms)
            .map_err(|e| format!("{PIPE_MARSEY_CONF}: {e}"))
    });
    let t_preload = std::thread::spawn(move || {
        pipes::send_named_pipe_utf8(PIPE_PRELOAD, &preload_data, timeout_ms)
            .map_err(|e| format!("{PIPE_PRELOAD}: {e}"))
    });
    let t_marsey = std::thread::spawn(move || {
        pipes::send_named_pipe_utf8(PIPE_MARSEY, &marsey_data, timeout_ms)
            .map_err(|e| format!("{PIPE_MARSEY}: {e}"))
    });
    let t_subverter = std::thread::spawn(move || {
        pipes::send_named_pipe_utf8(PIPE_SUBVERTER, &subverter_data, timeout_ms)
            .map_err(|e| format!("{PIPE_SUBVERTER}: {e}"))
    });

    let mut errors: Vec<String> = Vec::new();

    match t_conf.join() {
        Ok(Ok(())) => {}
        Ok(Err(e)) => errors.push(e),
        Err(_) => errors.push("MarseyConf pipe thread panic".to_string()),
    }
    match t_preload.join() {
        Ok(Ok(())) => {}
        Ok(Err(e)) => errors.push(e),
        Err(_) => errors.push("Preload pipe thread panic".to_string()),
    }
    match t_marsey.join() {
        Ok(Ok(())) => {}
        Ok(Err(e)) => errors.push(e),
        Err(_) => errors.push("Marsey patches pipe thread panic".to_string()),
    }
    match t_subverter.join() {
        Ok(Ok(())) => {}
        Ok(Err(e)) => errors.push(e),
        Err(_) => errors.push("Subverter pipe thread panic".to_string()),
    }

    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors.join("; "))
    }
}

fn load_enabled_patch_filenames(paths: &MarseyPaths) -> Result<Option<HashSet<String>>, String> {
    if !paths.patchlist_file.exists() {
        return Ok(None);
    }

    let text = std::fs::read_to_string(&paths.patchlist_file)
        .map_err(|e| format!("read {:?}: {e}", paths.patchlist_file))?;

    let mut set = HashSet::new();
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        set.insert(trimmed.to_string());
    }

    Ok(Some(set))
}

fn build_marsey_conf_string(ctx: &MarseyLaunchContext) -> String {
    // This string is parsed by Marsey.Utility.ReadConf(): key=value;key=value;...
    // Keep it strict: every segment must contain '='.
    let mut parts: Vec<String> = Vec::new();

    // Logging to SS14.Loader stdout (captured by our last-launch.log).
    parts.push("MARSEY_LOGGING=true".to_string());
    // Keep defaults quiet; enable when diagnosing patch issues.
    parts.push("MARSEY_LOADER_DEBUG=false".to_string());
    parts.push("MARSEY_LOADER_TRACE=false".to_string());
    parts.push("MARSEY_THROW_FAIL=false".to_string());
    parts.push("MARSEY_SEPARATE_LOGGER=false".to_string());
    parts.push("MARSEY_DISABLE_STRICT=false".to_string());

    parts.push("MARSEY_AUTODELETE_HWID=false".to_string());
    parts.push("MARSEY_DISABLE_PRESENCE=false".to_string());
    parts.push("MARSEY_FAKE_PRESENCE=false".to_string());
    parts.push("MARSEY_DUMP_ASSEMBLIES=false".to_string());
    parts.push(format!(
        "MARSEY_JAMMER={}",
        if ctx.disable_redial { "true" } else { "false" }
    ));
    parts.push("MARSEY_DISABLE_REC=false".to_string());

    // Backports are part of rewrite defaults; keep enabled.
    parts.push("MARSEY_BACKPORTS=true".to_string());
    parts.push("MARSEY_NO_ANY_BACKPORTS=false".to_string());

    parts.push(format!(
        "MARSEY_HIDE_LEVEL={}",
        conf_encode_value(&ctx.hide_level)
    ));
    parts.push("MARSEY_PATCHLESS=false".to_string());

    parts.push(format!(
        "MARSEY_ENGINE={}",
        conf_encode_value(&ctx.engine_version)
    ));
    parts.push(format!("MARSEY_FORKID={}", conf_encode_value(&ctx.fork_id)));

    parts.join(";")
}

fn scan_mods_dir(
    mods_dirs: &[PathBuf],
    enabled: &Option<HashSet<String>>,
) -> Result<ScannerOutput, String> {
    let mut out = ScannerOutput::default();
    if mods_dirs.is_empty() {
        return Ok(out);
    }

    let dlls = filter_enabled_mod_dlls(list_patch_dlls(mods_dirs)?, enabled);

    for p in dlls {
        let full = canonicalize_fallback(&p);
        let full_str = full.to_string_lossy().to_string();

        let Some(cls) = dotnet_metadata::try_classify_patch(&full) else {
            continue;
        };

        if cls.is_marsey {
            if cls.preload {
                out.preload.push(full_str.clone());
            } else {
                out.marsey.push(full_str.clone());
            }
        }
        if cls.is_subverter {
            out.subverter.push(full_str);
        }
    }

    out.preload.sort_by_key(|a| a.to_lowercase());
    out.marsey.sort_by_key(|a| a.to_lowercase());
    out.subverter.sort_by_key(|a| a.to_lowercase());

    Ok(out)
}

fn collect_enabled_mod_dlls(
    mods_dirs: &[PathBuf],
    enabled: &Option<HashSet<String>>,
) -> Result<Vec<String>, String> {
    let dlls = filter_enabled_mod_dlls(list_patch_dlls(mods_dirs)?, enabled);
    Ok(dlls
        .into_iter()
        .map(|p| canonicalize_fallback(&p).to_string_lossy().to_string())
        .collect())
}
