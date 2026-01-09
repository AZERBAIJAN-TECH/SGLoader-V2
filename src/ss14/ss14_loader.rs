use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

pub struct LoaderInstall {
    pub entrypoint: PathBuf,
    pub public_key: PathBuf,
    pub marsey_enabled: bool,
}

pub fn ensure_loader_installed(data_dir: &Path) -> Result<LoaderInstall, String> {
    const LOADER_BUILD_ID_REWRITE: &str = "rewrite-stable-2";

    let out_dir = data_dir.join("loader").join(platform_rid());
    fs::create_dir_all(&out_dir).map_err(|e| format!("создание каталога loader: {e}"))?;

    let public_key = out_dir.join("signing_key");
    let marker = out_dir.join("loader_source.txt");
    let build_id_file = out_dir.join("loader_build_id.txt");

    let exe = out_dir.join("SS14.Loader.exe");
    let dll = out_dir.join("SS14.Loader.dll");

    // Distribution path: prefer a packaged loader shipped next to SGLoader-V2.exe.
    // If present, copy it into the user data dir and use it.
    if let Some(packaged_dir) = packaged_loader_dir() {
        let packaged_exe = packaged_dir.join("SS14.Loader.exe");
        let packaged_dll = packaged_dir.join("SS14.Loader.dll");
        let packaged_key = packaged_dir.join("signing_key");

        if (packaged_exe.exists() || packaged_dll.exists()) && packaged_key.exists() {
            copy_dir_files(&packaged_dir, &out_dir)
                .map_err(|e| format!("копирование packaged SS14.Loader: {e}"))?;

            // Ensure key name matches what the launcher expects.
            fs::copy(&packaged_key, &public_key)
                .map_err(|e| format!("копирование signing_key: {e}"))?;

            let _ = fs::write(&marker, "rewrite");
            let _ = fs::write(&build_id_file, LOADER_BUILD_ID_REWRITE);

            let entrypoint = if exe.exists() {
                exe
            } else if dll.exists() {
                dll
            } else {
                return Err("после копирования не найден SS14.Loader.exe/.dll".to_string());
            };

            return Ok(LoaderInstall {
                entrypoint,
                public_key,
                marsey_enabled: true,
            });
        }
    }

    // Build/publish SS14.Loader from sources vendored in this repo.
    // We intentionally only support the rewrite submodule.
    let csproj = loader_csproj_path()?;
    let marsey_enabled = true;
    let desired_build_id = LOADER_BUILD_ID_REWRITE;

    if (exe.exists() || dll.exists()) && public_key.exists() {
        let installed_is_rewrite = fs::read_to_string(&marker)
            .ok()
            .map(|s| s.trim().eq_ignore_ascii_case("rewrite"))
            .unwrap_or(false);
        let build_ok = fs::read_to_string(&build_id_file)
            .ok()
            .map(|s| s.trim() == desired_build_id)
            .unwrap_or(false);

        if installed_is_rewrite == marsey_enabled && build_ok {
            return Ok(LoaderInstall {
                entrypoint: if exe.exists() { exe } else { dll },
                public_key,
                marsey_enabled,
            });
        }
    }

    // Preflight: SS14.Loader depends on Robust.LoaderApi submodule.
    // If it isn't initialized, dotnet will fail with confusing missing-namespace errors.
    if let Some(repo_root) = csproj.parent().and_then(|p| p.parent()) {
        let robust_api = repo_root
            .join("Robust.LoaderApi")
            .join("Robust.LoaderApi")
            .join("Robust.LoaderApi.csproj");
        if !robust_api.exists() {
            return Err(format!(
                "не найден {} (Robust.LoaderApi).\nПохоже, submodule не инициализирован.\nЗапусти: cd {} && git submodule update --init --recursive",
                robust_api.display(),
                repo_root.display()
            ));
        }
    }

    let mut cmd = Command::new("dotnet");
    cmd.arg("publish");
    cmd.arg(&csproj);
    cmd.arg("-c");
    cmd.arg("Release");
    cmd.arg("-r");
    cmd.arg(platform_rid());
    cmd.arg("--self-contained");
    // SGLoader-Rewrite targets newer .NET (net10 currently). Publishing self-contained avoids
    // runtime-missing crashes like: Could not load System.Runtime, Version=10.0.0.0.
    cmd.arg("true");
    cmd.arg("-o");
    cmd.arg(&out_dir);

    let status = cmd
        .status()
        .map_err(|e| format!("не удалось запустить dotnet для сборки SS14.Loader: {e}"))?;

    if !status.success() {
        return Err("dotnet publish SS14.Loader завершился с ошибкой".to_string());
    }

    // Copy signing key (public key) next to loader.
    let key_src = launcher_signing_key_path()?;
    fs::copy(&key_src, &public_key).map_err(|e| format!("копирование signing_key: {e}"))?;

    // Record which loader source produced this install.
    let _ = fs::write(&marker, "rewrite");
    let _ = fs::write(&build_id_file, desired_build_id);

    let entrypoint = if exe.exists() {
        exe
    } else if dll.exists() {
        dll
    } else {
        return Err("после publish не найден SS14.Loader.exe/.dll".to_string());
    };

    Ok(LoaderInstall {
        entrypoint,
        public_key,
        marsey_enabled,
    })
}

fn packaged_loader_dir() -> Option<PathBuf> {
    let exe_dir = std::env::current_exe().ok()?.parent()?.to_path_buf();
    Some(
        exe_dir
            .join("dependencies")
            .join("loader")
            .join(platform_rid()),
    )
}

fn copy_dir_files(from: &Path, to: &Path) -> std::io::Result<()> {
    fs::create_dir_all(to)?;

    for entry in fs::read_dir(from)? {
        let entry = entry?;
        let ty = entry.file_type()?;
        if !ty.is_file() {
            continue;
        }

        let src = entry.path();
        let Some(name) = src.file_name() else {
            continue;
        };

        let dst = to.join(name);
        // Best-effort overwrite.
        let _ = fs::remove_file(&dst);
        fs::copy(&src, &dst)?;
    }

    Ok(())
}

fn platform_rid() -> &'static str {
    // Minimal mapping; we currently only support Windows in this workspace.
    if cfg!(target_os = "windows") {
        if cfg!(target_arch = "x86_64") {
            return "win-x64";
        }
        if cfg!(target_arch = "aarch64") {
            return "win-arm64";
        }
        return "win-x86";
    }

    // Fallback.
    "win-x64"
}

fn loader_csproj_path() -> Result<PathBuf, String> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));

    let p_rewrite = root
        .join("third_party")
        .join("SGLoader-Rewrite")
        .join("SS14.Loader")
        .join("SS14.Loader.csproj");
    if p_rewrite.exists() {
        return Ok(p_rewrite);
    }

    Err(format!(
        "не найден SS14.Loader.csproj (нужен только rewrite-сабмодуль)\nПроверь: `git submodule update --init --recursive third_party/SGLoader-Rewrite`\n- rewrite: {}",
        p_rewrite.display()
    ))
}

fn launcher_signing_key_path() -> Result<PathBuf, String> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));

    let p_rewrite = root
        .join("third_party")
        .join("SGLoader-Rewrite")
        .join("SS14.Launcher")
        .join("signing_key");
    if p_rewrite.exists() {
        return Ok(p_rewrite);
    }

    Err(format!(
        "не найден signing_key (нужен только rewrite-сабмодуль)\nПроверь: `git submodule update --init --recursive third_party/SGLoader-Rewrite`\n- rewrite: {}",
        p_rewrite.display()
    ))
}
