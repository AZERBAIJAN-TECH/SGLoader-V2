use std::collections::HashMap;

use serde::Deserialize;

const ROBUST_BUILDS_MANIFEST_URLS: [&str; 2] = [
    "https://robust-builds.cdn.spacestation14.com/manifest.json",
    "https://robust-builds.fallback.cdn.spacestation14.com/manifest.json",
];

#[derive(Debug, Clone)]
pub struct RobustEngineBuild {
    pub requested_version: String,
    pub resolved_version: String,
    pub url: String,
    pub sha256: String,
    pub signature: String,
}

#[derive(Debug, Deserialize, Clone)]
struct VersionInfo {
    #[serde(default)]
    insecure: bool,

    #[serde(rename = "redirect")]
    redirect_version: Option<String>,

    platforms: HashMap<String, BuildInfo>,
}

#[derive(Debug, Deserialize, Clone)]
struct BuildInfo {
    url: String,
    sha256: String,

    #[serde(rename = "sig")]
    signature: String,
}

pub fn resolve_engine_build(engine_version: &str) -> Result<RobustEngineBuild, String> {
    let manifest = fetch_manifest()?;

    let (resolved_version, info) = follow_redirects(engine_version, &manifest)?;
    if info.insecure {
        return Err("указанная версия движка помечена как insecure".to_string());
    }

    let rid = pick_best_rid(info.platforms.keys().map(|s| s.as_str()).collect());
    let Some(rid) = rid else {
        return Err("для этой платформы нет сборки движка".to_string());
    };

    let build = info
        .platforms
        .get(&rid)
        .ok_or_else(|| "не удалось выбрать платформу для движка".to_string())?;

    Ok(RobustEngineBuild {
        requested_version: engine_version.to_string(),
        resolved_version,
        url: build.url.clone(),
        sha256: build.sha256.clone(),
        signature: build.signature.clone(),
    })
}

fn fetch_manifest() -> Result<HashMap<String, VersionInfo>, String> {
    let http = crate::launcher_mask::blocking_http_client_api()?;

    let mut last_err: Option<String> = None;
    for url in ROBUST_BUILDS_MANIFEST_URLS {
        match crate::http_config::blocking_send_idempotent_with_retry(|| http.get(url)) {
            Ok(resp) => match resp.error_for_status() {
                Ok(ok) => match ok.json::<HashMap<String, VersionInfo>>() {
                    Ok(m) => return Ok(m),
                    Err(e) => last_err = Some(format!("robust manifest parse: {e}")),
                },
                Err(e) => last_err = Some(format!("robust manifest status: {e}")),
            },
            Err(e) => last_err = Some(format!("robust manifest request: {e}")),
        }
    }

    Err(last_err.unwrap_or_else(|| "не удалось загрузить robust manifest".to_string()))
}

fn follow_redirects(
    requested_version: &str,
    manifest: &HashMap<String, VersionInfo>,
) -> Result<(String, VersionInfo), String> {
    let mut version = requested_version.to_string();

    let mut info = manifest
        .get(&version)
        .cloned()
        .ok_or_else(|| "engine_version отсутствует в robust manifest".to_string())?;

    // Follow redirects.
    while let Some(next) = info.redirect_version.clone() {
        version = next;
        info = manifest
            .get(&version)
            .cloned()
            .ok_or_else(|| "redirect engine_version отсутствует в robust manifest".to_string())?;
    }

    Ok((version, info))
}

fn pick_best_rid(available: Vec<&str>) -> Option<String> {
    // Minimal RID selection mirroring SS14.Launcher behavior.
    // Prefer exact matches for current OS/arch.
    let os = std::env::consts::OS;
    let arch = std::env::consts::ARCH;

    let mut candidates: Vec<&str> = Vec::new();

    match (os, arch) {
        ("windows", "x86_64") => candidates.extend(["win-x64", "win-x86"]),
        ("windows", "x86") => candidates.extend(["win-x86", "win-x64"]),
        ("windows", "aarch64") => candidates.extend(["win-arm64", "win-x64"]),
        ("linux", "x86_64") => candidates.extend(["linux-x64"]),
        ("linux", "aarch64") => candidates.extend(["linux-arm64"]),
        ("macos", "aarch64") => candidates.extend(["osx-arm64"]),
        ("macos", "x86_64") => candidates.extend(["osx-x64"]),
        _ => {}
    }

    for c in candidates {
        if available.iter().any(|x| x.eq_ignore_ascii_case(c)) {
            // Use canonical casing from manifest key if possible.
            if let Some(actual) = available.iter().find(|x| x.eq_ignore_ascii_case(c)) {
                return Some((*actual).to_string());
            }
            return Some(c.to_string());
        }
    }

    // Fallback: first available.
    available.first().map(|s| (*s).to_string())
}
