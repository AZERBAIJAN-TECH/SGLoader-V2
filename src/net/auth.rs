use std::fmt;

use chrono::{DateTime, Duration, Utc};
use reqwest::{Client, StatusCode};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

const AUTH_BASE_URLS: &[&str] = &[
    "https://auth.spacestation14.com/",
    "https://auth.fallback.spacestation14.com/",
];

#[derive(Clone)]
pub struct AuthApi {
    client: Client,
}

impl Default for AuthApi {
    fn default() -> Self {
        Self::new()
    }
}

impl AuthApi {
    pub fn new() -> Self {
        Self {
            client: crate::http_config::build_async_client(crate::http_config::HttpProfile::Api)
                .unwrap_or_else(|_| Client::new()),
        }
    }

    pub async fn authenticate(
        &self,
        username: String,
        password: String,
    ) -> Result<AuthenticateResult, AuthError> {
        let request = AuthenticateRequest {
            username: Some(username),
            user_id: None,
            password,
            tfa_code: None,
        };

        self.authenticate_inner(request).await
    }

    async fn authenticate_inner(
        &self,
        request: AuthenticateRequest,
    ) -> Result<AuthenticateResult, AuthError> {
        let mut last_error: Option<AuthError> = None;

        for base in AUTH_BASE_URLS {
            let auth_url = format!("{}api/auth/authenticate", base);
            let response = self.client.post(auth_url).json(&request).send().await;

            let response = match response {
                Ok(resp) => resp,
                Err(err) => {
                    last_error = Some(AuthError::Network(err.to_string()));
                    continue;
                }
            };

            match response.status() {
                StatusCode::OK => {
                    let parsed = response
                        .json::<AuthenticateResponse>()
                        .await
                        .map_err(|err| {
                            AuthError::Parse(format!("Не удалось разобрать ответ: {err}"))
                        })?;

                    let login_info = LoginInfo {
                        user_id: parsed.user_id,
                        username: parsed.username,
                        token: LoginToken {
                            token: parsed.token,
                            expire_time: parsed.expire_time,
                        },
                    };

                    return Ok(AuthenticateResult::Success(login_info));
                }
                StatusCode::UNAUTHORIZED => {
                    let parsed =
                        response
                            .json::<AuthenticateDenyResponse>()
                            .await
                            .map_err(|err| {
                                AuthError::Parse(format!("Не удалось разобрать ошибку: {err}"))
                            })?;

                    return Ok(AuthenticateResult::Failure {
                        errors: parsed.errors,
                        code: parsed.code,
                    });
                }
                status => {
                    last_error = Some(AuthError::UnexpectedStatus(status));
                }
            }
        }

        Err(last_error.unwrap_or(AuthError::Network(
            "Не удалось связаться с auth сервером".to_string(),
        )))
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct AuthenticateRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    username: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    user_id: Option<Uuid>,
    password: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    tfa_code: Option<String>,
}

#[derive(Debug, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct AuthenticateResponse {
    pub token: String,
    pub username: String,
    pub user_id: Uuid,
    pub expire_time: DateTime<Utc>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct AuthenticateDenyResponse {
    pub errors: Vec<String>,
    pub code: AuthenticateDenyResponseCode,
}

#[derive(Debug, Deserialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "PascalCase")]
pub enum AuthenticateDenyResponseCode {
    None,
    InvalidCredentials,
    AccountUnconfirmed,
    TfaRequired,
    TfaInvalid,
    AccountLocked,
    #[serde(other)]
    UnknownError,
}

#[derive(Debug, Clone)]
pub enum AuthenticateResult {
    Success(LoginInfo),
    Failure {
        errors: Vec<String>,
        code: AuthenticateDenyResponseCode,
    },
}

#[derive(Debug, Clone)]
pub struct LoginToken {
    pub token: String,
    pub expire_time: DateTime<Utc>,
}

impl LoginToken {
    pub fn is_time_expired(&self) -> bool {
        self.expire_time <= Utc::now()
    }

    pub fn should_refresh(&self) -> bool {
        self.expire_time <= Utc::now() + Duration::days(15)
    }
}

#[derive(Debug, Clone)]
pub struct LoginInfo {
    pub user_id: Uuid,
    pub username: String,
    pub token: LoginToken,
}

#[derive(Debug, Clone)]
pub enum AuthError {
    Network(String),
    UnexpectedStatus(StatusCode),
    Parse(String),
}

impl fmt::Display for AuthError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AuthError::Network(err) => write!(f, "сетевая ошибка: {err}"),
            AuthError::UnexpectedStatus(code) => write!(f, "неожиданный статус сервера: {code}"),
            AuthError::Parse(err) => write!(f, "ошибка разбора ответа: {err}"),
        }
    }
}

impl std::error::Error for AuthError {}
