//! Parent environment values deliberately passed through cleared children.

use std::ffi::OsString;
use std::path::PathBuf;

use super::{EnvPolicy, proxy_passthrough};

pub(super) fn cleared_parent_passthrough(
    policy: EnvPolicy,
    parent: &[(OsString, OsString)],
) -> Vec<(OsString, OsString)> {
    match policy {
        // Mise needs runner discovery plus its selected owned cache.
        EnvPolicy::GitHubToken => {
            let mut env: Vec<_> = parent
                .iter()
                .filter(|(key, _)| key == "PATH" || key == "HOME")
                .cloned()
                .collect();
            let last_env = |name: &str| {
                parent
                    .iter()
                    .rfind(|(key, _)| key == name)
                    .map(|(_, value)| value.clone())
            };
            let data_dir = last_env("MISE_DATA_DIR")
                .filter(|value| !value.is_empty())
                .or_else(|| {
                    last_env("XDG_DATA_HOME")
                        .filter(|value| !value.is_empty())
                        .map(|value| PathBuf::from(value).join("mise").into_os_string())
                });
            env.extend(data_dir.map(|value| (OsString::from("MISE_DATA_DIR"), value)));
            env
        }
        policy if policy.allows_proxy_passthrough() => proxy_passthrough(parent),
        _ => Vec::new(),
    }
}
