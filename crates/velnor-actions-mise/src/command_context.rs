// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{EnvPolicy, MiseError, OsString, PathBuf, proxy_passthrough};

pub(super) fn with_pinned_git(
    env: &[(OsString, OsString)],
    git: &crate::release_git_pin::VerifiedGit,
    invalid: impl Fn() -> MiseError,
) -> Result<Vec<(OsString, OsString)>, MiseError> {
    let root = git.exec_path();
    if !root.is_absolute() || git.executable().parent() != Some(root) || !root.is_dir() {
        return Err(invalid());
    }
    let mut result = env.to_vec();
    let path = result
        .iter_mut()
        .find(|(key, _)| key == "PATH")
        .ok_or_else(&invalid)?;
    let mut entries = vec![root.to_path_buf()];
    entries.extend(std::env::split_paths(&path.1));
    path.1 = std::env::join_paths(entries).map_err(|_| invalid())?;
    result.push(("GIT_EXEC_PATH".into(), root.as_os_str().to_owned()));
    Ok(result)
}

pub(super) fn cleared_parent_passthrough(
    policy: EnvPolicy,
    parent: &[(OsString, OsString)],
) -> Vec<(OsString, OsString)> {
    match policy {
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
