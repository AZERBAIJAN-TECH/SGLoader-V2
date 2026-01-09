use std::fs;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

const SETTINGS_FILE_NAME: &str = "settings.json";

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct LauncherSettings {
    pub security: SecuritySettings,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecuritySettings {
    pub hide_level: HideLevel,
    pub auto_login: bool,
    pub disable_redial: bool,
    pub autodelete_hwid: bool,
}

impl Default for SecuritySettings {
    fn default() -> Self {
        Self {
            hide_level: HideLevel::Medium,
            auto_login: true,
            disable_redial: false,
            autodelete_hwid: false,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HideLevel {
    Disabled,
    Low,
    Medium,
    High,
    Maximum,
}

impl HideLevel {
    pub fn label_ru(self) -> &'static str {
        match self {
            HideLevel::Disabled => "Отключен",
            HideLevel::Low => "Низкий",
            HideLevel::Medium => "Средний",
            HideLevel::High => "Высокий",
            HideLevel::Maximum => "Максимальный",
        }
    }

    pub fn to_marsey_value(self) -> &'static str {
        // Marseyloader HideLevel enum names:
        // Disabled, Duplicit, Normal, Explicit, Unconditional
        match self {
            HideLevel::Disabled => "Disabled",
            HideLevel::Low => "Duplicit",
            HideLevel::Medium => "Normal",
            HideLevel::High => "Explicit",
            HideLevel::Maximum => "Unconditional",
        }
    }

    pub fn from_key(key: &str) -> Option<Self> {
        match key {
            "disabled" => Some(HideLevel::Disabled),
            "low" => Some(HideLevel::Low),
            "medium" => Some(HideLevel::Medium),
            "high" => Some(HideLevel::High),
            "maximum" => Some(HideLevel::Maximum),
            _ => None,
        }
    }

    pub fn as_key(self) -> &'static str {
        match self {
            HideLevel::Disabled => "disabled",
            HideLevel::Low => "low",
            HideLevel::Medium => "medium",
            HideLevel::High => "high",
            HideLevel::Maximum => "maximum",
        }
    }
}

pub fn load_settings() -> Result<LauncherSettings, String> {
    let path = settings_file_path()?;
    let contents = match fs::read_to_string(&path) {
        Ok(data) => data,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            return Ok(LauncherSettings::default());
        }
        Err(err) => return Err(format!("не удалось прочитать настройки: {err}")),
    };

    serde_json::from_str(&contents).map_err(|e| format!("не удалось разобрать настройки: {e}"))
}

pub fn save_settings(settings: &LauncherSettings) -> Result<(), String> {
    let dir = crate::app_paths::data_dir()?;
    fs::create_dir_all(&dir).map_err(|e| format!("mkdir настройки: {e}"))?;

    let path = settings_file_path()?;
    let json =
        serde_json::to_string_pretty(settings).map_err(|e| format!("serialize настройки: {e}"))?;
    fs::write(&path, json).map_err(|e| format!("запись настроек: {e}"))?;

    Ok(())
}

fn settings_file_path() -> Result<PathBuf, String> {
    Ok(crate::app_paths::data_dir()?.join(SETTINGS_FILE_NAME))
}
