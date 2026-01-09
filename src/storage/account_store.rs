use std::fs;
use std::path::PathBuf;

use base64::{Engine as _, engine::general_purpose};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::auth::LoginInfo;
use crate::secure_token::{decrypt_token, encrypt_token};

const LOGIN_FILE_NAME: &str = "logins.json";

pub fn load_saved_logins() -> Result<Vec<LoginInfo>, String> {
    let stored = read_logins_file()?;
    Ok(stored
        .items
        .into_iter()
        .filter_map(decode_login)
        .collect())
}

pub fn load_saved_login() -> Result<Option<LoginInfo>, String> {
    let stored = read_logins_file()?;

    if let Some(active_id) = stored.active_user_id {
        if let Some(info) = stored
            .items
            .iter()
            .find(|i| i.user_id == active_id)
            .cloned()
            .and_then(decode_login)
        {
            return Ok(Some(info));
        }
    }

    Ok(stored.items.into_iter().filter_map(decode_login).next())
}

pub fn save_login(login: &LoginInfo) -> Result<(), String> {
    let mut stored_file = read_logins_file().unwrap_or_default();

    let encrypted = encrypt_token(login.token.token.as_bytes())
        .map_err(|e| format!("ошибка шифрования токена: {e}"))?;
    let token_enc = general_purpose::STANDARD.encode(encrypted);

    let stored_login = StoredLogin {
        user_id: login.user_id,
        username: login.username.clone(),
        token_enc,
        expire_time: login.token.expire_time,
    };

    let stored_user_id = stored_login.user_id;

    if let Some(existing) = stored_file
        .items
        .iter_mut()
        .find(|i| i.user_id == stored_user_id)
    {
        *existing = stored_login;
    } else {
        stored_file.items.push(stored_login);
    }

    stored_file.active_user_id = Some(login.user_id);

    write_logins_file(&stored_file)
}

pub fn set_active_login(user_id: uuid::Uuid) -> Result<(), String> {
    let mut stored = read_logins_file()?;
    if !stored.items.iter().any(|i| i.user_id == user_id) {
        return Err("указанный аккаунт не найден".to_string());
    }
    stored.active_user_id = Some(user_id);
    write_logins_file(&stored)
}

pub fn remove_login(user_id: uuid::Uuid) -> Result<(), String> {
    let mut stored = read_logins_file()?;
    let before = stored.items.len();
    stored.items.retain(|i| i.user_id != user_id);
    if stored.items.len() == before {
        return Err("указанный аккаунт не найден".to_string());
    }
    if stored.active_user_id == Some(user_id) {
        stored.active_user_id = None;
    }
    write_logins_file(&stored)
}

pub fn clear_saved_logins() -> Result<(), String> {
    let path = login_file_path()?;
    match fs::remove_file(&path) {
        Ok(_) => Ok(()),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(err) => Err(format!("не удалось удалить файл логинов: {err}")),
    }
}

fn login_file_path() -> Result<PathBuf, String> {
    Ok(crate::app_paths::data_dir()?.join(LOGIN_FILE_NAME))
}

fn read_logins_file() -> Result<StoredLoginsFileV2, String> {
    let path = login_file_path()?;
    let contents = match fs::read_to_string(&path) {
        Ok(data) => data,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            return Ok(StoredLoginsFileV2::default());
        }
        Err(err) => return Err(format!("не удалось прочитать файл логинов: {err}")),
    };

    let parsed: StoredLoginsFile = serde_json::from_str(&contents)
        .map_err(|err| format!("не удалось разобрать логины: {err}"))?;
    Ok(match parsed {
        StoredLoginsFile::V1(items) => StoredLoginsFileV2 {
            active_user_id: None,
            items,
        },
        StoredLoginsFile::V2(v2) => v2,
    })
}

fn write_logins_file(stored: &StoredLoginsFileV2) -> Result<(), String> {
    let dir = crate::app_paths::data_dir()?;
    fs::create_dir_all(&dir)
        .map_err(|err| format!("не удалось создать каталог для логинов: {err}"))?;

    let path = login_file_path()?;
    let serialized = serde_json::to_string_pretty(stored)
        .map_err(|err| format!("не удалось сериализовать логины: {err}"))?;
    fs::write(&path, serialized).map_err(|err| format!("не удалось записать логины: {err}"))?;
    Ok(())
}

fn decode_login(item: StoredLogin) -> Option<LoginInfo> {
    let encrypted = general_purpose::STANDARD.decode(item.token_enc).ok()?;
    let token = decrypt_token(&encrypted).ok()?;
    Some(LoginInfo {
        user_id: item.user_id,
        username: item.username,
        token: crate::auth::LoginToken {
            token,
            expire_time: item.expire_time,
        },
    })
}

#[derive(Debug, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct StoredLoginsFileV2 {
    #[serde(skip_serializing_if = "Option::is_none")]
    active_user_id: Option<uuid::Uuid>,
    #[serde(default)]
    items: Vec<StoredLogin>,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum StoredLoginsFile {
    V1(Vec<StoredLogin>),
    V2(StoredLoginsFileV2),
}

#[derive(Debug, Serialize, Deserialize, Default, Clone)]
struct StoredLogin {
    user_id: uuid::Uuid,
    username: String,
    token_enc: String,
    expire_time: DateTime<Utc>,
}
