use url::Url;

const DEFAULT_SS14_PORT: u16 = 1212;

pub fn parse_ss14_uri(address: &str) -> Result<Url, String> {
    let mut address = address.trim().to_string();
    if !address.contains("://") {
        address = format!("ss14://{address}");
    }

    let uri = Url::parse(&address).map_err(|_| "неверный адрес сервера".to_string())?;

    match uri.scheme() {
        "ss14" | "ss14s" => {}
        _ => return Err("поддерживаются только ss14:// и ss14s://".to_string()),
    }

    if uri.host_str().is_none() {
        return Err("в адресе сервера отсутствует host".to_string());
    }

    Ok(uri)
}

pub fn server_api_base(ss14_uri: &Url) -> Result<Url, String> {
    let host = ss14_uri
        .host_str()
        .ok_or_else(|| "в адресе сервера отсутствует host".to_string())?;

    let scheme = match ss14_uri.scheme() {
        "ss14" => "http",
        "ss14s" => "https",
        _ => return Err("wrong scheme".to_string()),
    };

    let mut base = format!("{scheme}://{host}");

    // For ss14:// default port is 1212. For ss14s:// default is 443 (handled by https if omitted).
    match ss14_uri.scheme() {
        "ss14" => {
            let port = ss14_uri.port().unwrap_or(DEFAULT_SS14_PORT);
            base.push_str(&format!(":{port}"));
        }
        "ss14s" => {
            if let Some(port) = ss14_uri.port() {
                base.push_str(&format!(":{port}"));
            }
        }
        _ => {}
    }

    let mut path = ss14_uri.path().to_string();
    if !path.ends_with('/') {
        path.push('/');
    }

    base.push_str(&path);
    Url::parse(&base).map_err(|e| format!("api url: {e}"))
}

pub fn server_info_url(ss14_uri: &Url) -> Result<Url, String> {
    let base = server_api_base(ss14_uri)?;
    base.join("info").map_err(|e| format!("info url: {e}"))
}

pub fn server_status_url(ss14_uri: &Url) -> Result<Url, String> {
    let base = server_api_base(ss14_uri)?;
    base.join("status").map_err(|e| format!("status url: {e}"))
}

pub fn server_selfhosted_client_zip_url(ss14_uri: &Url) -> Result<Url, String> {
    let base = server_api_base(ss14_uri)?;
    base.join("client.zip")
        .map_err(|e| format!("client.zip url: {e}"))
}
