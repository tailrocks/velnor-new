// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::{collections::BTreeSet, ffi::OsString, path::Path};

use super::{EnvPolicy, IsolatedCommand, is_safe_credential_value};
use crate::MiseError;

#[derive(Clone, Copy)]
pub enum CratePublishCredential<'a> {
    Trusted {
        request_url: &'a str,
        request_token: &'a str,
    },
    Bootstrap {
        cargo_token: &'a str,
    },
}

impl std::fmt::Debug for CratePublishCredential<'_> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Trusted { .. } => {
                formatter.write_str("CratePublishCredential::Trusted(<redacted>)")
            }
            Self::Bootstrap { .. } => {
                formatter.write_str("CratePublishCredential::Bootstrap(<redacted>)")
            }
        }
    }
}

const ALLOWED_ENV: &[&str] = &[
    "PATH",
    "CARGO",
    "RUSTC",
    "RUSTDOC",
    "HOME",
    "CARGO_HOME",
    "RUSTUP_HOME",
    "RUSTUP_AUTO_INSTALL",
    "RUNNER_TEMP",
    "GITHUB_WORKSPACE",
    "GITHUB_REPOSITORY",
    "GITHUB_SHA",
    "GITHUB_REF",
    "GITHUB_EVENT_NAME",
    "GITHUB_EVENT_PATH",
    "GITHUB_SERVER_URL",
    "GITHUB_API_URL",
    "GITHUB_RUN_ID",
    "GITHUB_RUN_ATTEMPT",
];

fn invalid() -> MiseError {
    MiseError::InvalidStepInput {
        field: "crate_publish".to_owned(),
        value: "invalid_input".to_owned(),
    }
}

fn runner_oidc_url(url: &str) -> bool {
    let Some(rest) = url.strip_prefix("https://") else {
        return false;
    };
    let Some((host, path)) = rest.split_once('/') else {
        return false;
    };
    !path.is_empty()
        && !url.chars().any(char::is_control)
        && !url.contains(['\\', '#', ' '])
        && host.ends_with(".actions.githubusercontent.com")
        && host.split('.').all(|label| {
            !label.is_empty()
                && label
                    .bytes()
                    .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
        })
}

impl IsolatedCommand {
    pub fn crate_publish(
        program: &Path,
        args: Vec<OsString>,
        cwd: &Path,
        tool_env: &[(OsString, OsString)],
        git_token: &str,
        credential: CratePublishCredential<'_>,
    ) -> Result<Self, MiseError> {
        if !program.is_absolute() || !cwd.is_absolute() || !is_safe_credential_value(git_token) {
            return Err(invalid());
        }
        let secret = match &credential {
            CratePublishCredential::Trusted { request_token, .. } => *request_token,
            CratePublishCredential::Bootstrap { cargo_token } => *cargo_token,
        };
        if args.iter().any(|arg| {
            let arg = arg.to_string_lossy();
            arg.contains(git_token) || (!secret.is_empty() && arg.contains(secret))
        }) {
            return Err(invalid());
        }
        let mut names = BTreeSet::new();
        for (key, value) in tool_env {
            let key = key.to_str().ok_or_else(invalid)?;
            if !ALLOWED_ENV.contains(&key) || !names.insert(key) || value.is_empty() {
                return Err(invalid());
            }
            if (key == "GITHUB_API_URL" && value != "https://api.github.com")
                || (key == "GITHUB_SERVER_URL" && value != "https://github.com")
            {
                return Err(invalid());
            }
        }
        for key in [
            "PATH",
            "CARGO",
            "RUSTC",
            "RUSTDOC",
            "HOME",
            "CARGO_HOME",
            "RUSTUP_HOME",
        ] {
            if !names.contains(key) {
                return Err(invalid());
            }
        }
        let mut extra_env = tool_env.to_vec();
        extra_env.push(("GIT_TOKEN".into(), git_token.into()));
        match credential {
            CratePublishCredential::Trusted {
                request_url,
                request_token,
            } => {
                if !runner_oidc_url(request_url) || !is_safe_credential_value(request_token) {
                    return Err(invalid());
                }
                extra_env.extend([
                    ("GITHUB_ACTIONS".into(), "true".into()),
                    ("ACTIONS_ID_TOKEN_REQUEST_URL".into(), request_url.into()),
                    (
                        "ACTIONS_ID_TOKEN_REQUEST_TOKEN".into(),
                        request_token.into(),
                    ),
                ]);
            }
            CratePublishCredential::Bootstrap { cargo_token } => {
                if !is_safe_credential_value(cargo_token) {
                    return Err(invalid());
                }
                extra_env.push(("CARGO_REGISTRY_TOKEN".into(), cargo_token.into()));
            }
        }
        Ok(Self {
            program: program.as_os_str().to_owned(),
            args,
            cwd: Some(cwd.to_path_buf()),
            extra_env,
            policy: EnvPolicy::QualifiedProbe,
            operator_guarded: false,
        })
    }
}
