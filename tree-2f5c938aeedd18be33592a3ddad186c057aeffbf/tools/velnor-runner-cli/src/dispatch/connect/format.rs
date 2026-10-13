use super::ConnectError;

pub(super) fn credential_reference(host_platform: &str) -> &'static str {
    if host_platform == "linux" {
        "systemd-credential:github-token"
    } else {
        "keychain:com.tailrocks.velnor.host/velnor-host"
    }
}

pub(super) fn toml_string(value: &str) -> Result<String, ConnectError> {
    serde_json::to_string(value).map_err(|_| ConnectError::Config)
}
