use serde::Deserialize;

#[derive(Debug, Deserialize, Clone)]
pub struct ServerInfo {
    #[serde(rename = "connect_address")]
    pub connect_address: Option<String>,

    #[serde(rename = "build")]
    pub build_information: Option<ServerBuildInformation>,

    #[serde(rename = "auth")]
    pub auth_information: ServerAuthInformation,

    #[serde(rename = "desc")]
    pub desc: Option<String>,

    #[serde(rename = "privacy_policy")]
    pub privacy_policy: Option<ServerPrivacyPolicyInfo>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct ServerAuthInformation {
    #[serde(rename = "mode")]
    pub mode: AuthMode,

    #[serde(rename = "public_key")]
    pub public_key: String,
}

#[derive(Debug, Deserialize, Clone)]
pub struct ServerBuildInformation {
    #[serde(rename = "download_url")]
    pub download_url: Option<String>,

    #[serde(rename = "manifest_url")]
    pub manifest_url: Option<String>,

    #[serde(rename = "manifest_download_url")]
    pub manifest_download_url: Option<String>,

    #[serde(rename = "engine_version")]
    pub engine_version: String,

    #[serde(rename = "version")]
    pub version: String,

    #[serde(rename = "fork_id")]
    pub fork_id: String,

    #[serde(rename = "hash")]
    pub hash: Option<String>,

    #[serde(rename = "manifest_hash")]
    pub manifest_hash: Option<String>,

    #[serde(rename = "acz")]
    pub acz: bool,
}

#[derive(Debug, Deserialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum AuthMode {
    #[serde(alias = "Optional")]
    Optional,
    #[serde(alias = "Required")]
    Required,
    #[serde(alias = "Disabled")]
    Disabled,
}

#[derive(Debug, Deserialize, Clone)]
pub struct ServerPrivacyPolicyInfo {
    #[serde(rename = "link")]
    pub link: String,
    #[serde(rename = "identifier")]
    pub identifier: String,
    #[serde(rename = "version")]
    pub version: String,
}
