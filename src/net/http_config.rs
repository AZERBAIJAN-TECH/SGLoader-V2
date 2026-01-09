use std::time::Duration;

use reqwest::header::HeaderMap;

#[derive(Debug, Clone, Copy)]
pub enum HttpProfile {
    /// Short-lived JSON/API calls.
    Api,
    /// Large downloads (ZIPs, manifests, etc.).
    Download,
}

fn connect_timeout(profile: HttpProfile) -> Duration {
    match profile {
        // Keep connect reasonably small; failures should surface quickly.
        HttpProfile::Api | HttpProfile::Download => Duration::from_secs(10),
    }
}

fn request_timeout(profile: HttpProfile) -> Duration {
    match profile {
        // For API calls, fail fast.
        HttpProfile::Api => Duration::from_secs(20),
        // For large downloads, allow long transfers.
        HttpProfile::Download => Duration::from_secs(60 * 10),
    }
}

pub fn build_async_client(profile: HttpProfile) -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .connect_timeout(connect_timeout(profile))
        .timeout(request_timeout(profile))
        .build()
        .map_err(|e| format!("init http: {e}"))
}

pub fn build_async_client_with_headers(
    headers: HeaderMap,
    profile: HttpProfile,
) -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .default_headers(headers)
        .connect_timeout(connect_timeout(profile))
        .timeout(request_timeout(profile))
        .build()
        .map_err(|e| format!("init http: {e}"))
}

pub fn build_blocking_client_with_headers(
    headers: HeaderMap,
    profile: HttpProfile,
) -> Result<reqwest::blocking::Client, String> {
    reqwest::blocking::Client::builder()
        .default_headers(headers)
        .connect_timeout(connect_timeout(profile))
        .timeout(request_timeout(profile))
        .build()
        .map_err(|e| format!("init http: {e}"))
}

fn should_retry_status(status: reqwest::StatusCode) -> bool {
    // Keep retries conservative and focused on common transient statuses.
    status == reqwest::StatusCode::REQUEST_TIMEOUT
        || status == reqwest::StatusCode::TOO_MANY_REQUESTS
        || status == reqwest::StatusCode::BAD_GATEWAY
        || status == reqwest::StatusCode::SERVICE_UNAVAILABLE
        || status == reqwest::StatusCode::GATEWAY_TIMEOUT
        || status.is_server_error()
}

fn should_retry_error(err: &reqwest::Error) -> bool {
    err.is_timeout() || err.is_connect()
}

fn backoff_delay(attempt: usize) -> Duration {
    // attempt: 0 -> 250ms, 1 -> 750ms, 2 -> 1500ms
    match attempt {
        0 => Duration::from_millis(250),
        1 => Duration::from_millis(750),
        _ => Duration::from_millis(1500),
    }
}

fn retry_after(headers: &HeaderMap) -> Option<Duration> {
    let raw = headers.get(reqwest::header::RETRY_AFTER)?.to_str().ok()?;
    let secs = raw.trim().parse::<u64>().ok()?;
    // Cap to avoid hanging too long.
    Some(Duration::from_secs(secs.min(5)))
}

/// Sends an idempotent **blocking** request with limited retries.
///
/// Retries on connect/timeout errors and on transient HTTP statuses (429, 5xx, 408).
pub fn blocking_send_idempotent_with_retry<F>(
    mut build: F,
) -> Result<reqwest::blocking::Response, reqwest::Error>
where
    F: FnMut() -> reqwest::blocking::RequestBuilder,
{
    const MAX_RETRIES: usize = 2;

    for attempt in 0..=MAX_RETRIES {
        let resp = build().send();
        match resp {
            Ok(resp) => {
                if attempt < MAX_RETRIES && should_retry_status(resp.status()) {
                    let delay =
                        retry_after(resp.headers()).unwrap_or_else(|| backoff_delay(attempt));
                    std::thread::sleep(delay);
                    continue;
                }
                return Ok(resp);
            }
            Err(err) => {
                if attempt < MAX_RETRIES && should_retry_error(&err) {
                    std::thread::sleep(backoff_delay(attempt));
                    continue;
                }
                return Err(err);
            }
        }
    }

    unreachable!()
}

/// Sends an idempotent **async** request with limited retries.
///
/// Retries on connect/timeout errors and on transient HTTP statuses (429, 5xx, 408).
pub async fn async_send_idempotent_with_retry<F>(
    mut build: F,
) -> Result<reqwest::Response, reqwest::Error>
where
    F: FnMut() -> reqwest::RequestBuilder,
{
    const MAX_RETRIES: usize = 2;

    for attempt in 0..=MAX_RETRIES {
        let resp = build().send().await;
        match resp {
            Ok(resp) => {
                if attempt < MAX_RETRIES && should_retry_status(resp.status()) {
                    let delay =
                        retry_after(resp.headers()).unwrap_or_else(|| backoff_delay(attempt));
                    tokio::time::sleep(delay).await;
                    continue;
                }
                return Ok(resp);
            }
            Err(err) => {
                if attempt < MAX_RETRIES && should_retry_error(&err) {
                    tokio::time::sleep(backoff_delay(attempt)).await;
                    continue;
                }
                return Err(err);
            }
        }
    }

    unreachable!()
}
