// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde::Deserialize;
use velnor_model::{MbxRemote, RepoView, RustCiConfig, RustRoot};

use crate::config::CONFIG_PATH;
use crate::error::{DetectError, Reason};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RawCache {
    mbx_remote: Option<String>,
}

pub(crate) fn mbx_remote(raw: Option<RawCache>) -> Result<MbxRemote, Reason> {
    match raw.and_then(|cache| cache.mbx_remote).as_deref() {
        None => Ok(MbxRemote::Off),
        Some("github-actions") => Ok(MbxRemote::GithubActions),
        Some(other) => Err(Reason::InvalidValue {
            key: "cache.mbx_remote",
            value: other.to_owned(),
        }),
    }
}

/// The mbx cache restores objects on fork pull requests, so a repository whose
/// dependencies are not proven public must not opt in.
pub(crate) fn private_sources(
    view: &RepoView,
    roots: &[RustRoot],
    config: &RustCiConfig,
) -> Result<Option<String>, DetectError> {
    let private = crate::private_sources::private_sources(view, roots);
    match (config.mbx_remote, &private) {
        (MbxRemote::GithubActions, Some(cause)) => Err(DetectError::invalid(
            &CONFIG_PATH,
            Reason::MbxRemotePrivate(cause.clone()),
        )),
        _ => Ok(private),
    }
}
