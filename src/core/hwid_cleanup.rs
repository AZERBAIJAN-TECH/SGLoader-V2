#[cfg(windows)]
pub fn clear_robust_hkcu_values() -> Result<(), String> {
    use std::io;

    use winreg::RegKey;
    use winreg::enums::{HKEY_CURRENT_USER, KEY_READ, KEY_WRITE};

    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let key_path = r"Software\Space Wizards\Robust";

    let key = match hkcu.open_subkey_with_flags(key_path, KEY_READ | KEY_WRITE) {
        Ok(k) => k,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(format!("не удалось открыть HKCU\\{key_path}: {e}")),
    };

    let names: Vec<String> = key
        .enum_values()
        .filter_map(|res| res.ok().map(|(name, _value)| name))
        .collect();

    for name in names {
        if let Err(e) = key.delete_value(&name) {
            return Err(format!("не удалось удалить значение реестра '{name}': {e}"));
        }
    }

    Ok(())
}

#[cfg(not(windows))]
pub fn clear_robust_hkcu_values() -> Result<(), String> {
    Ok(())
}
