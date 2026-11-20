// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde::Deserialize;
use velnor_model::{MbxMode, MbxRemote, MbxStore, RepoView, RustCiConfig, RustRoot};

use crate::config::CONFIG_PATH;
use crate::error::{DetectError, Reason};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RawCache {
    mbx_remote: Option<String>,
    mbx: Option<bool>,
}

pub(crate) fn mbx_remote(raw: Option<&RawCache>) -> Result<MbxRemote, Reason> {
    match raw.and_then(|cache| cache.mbx_remote.as_deref()) {
        None => Ok(MbxRemote::Off),
        Some("github-actions") => Ok(MbxRemote::GithubActions),
        Some(other) => Err(Reason::InvalidValue {
            key: "cache.mbx_remote",
            value: other.to_owned(),
        }),
    }
}

pub(crate) fn mbx_settings(
    raw: Option<&RawCache>,
    shared_store: Option<bool>,
) -> Result<(MbxRemote, MbxStore, MbxMode), Reason> {
    let remote = mbx_remote(raw)?;
    let store = shared_store.unwrap_or(false).into();
    Ok((remote, store, mbx(raw, remote, store)?))
}

fn mbx(raw: Option<&RawCache>, remote: MbxRemote, store: MbxStore) -> Result<MbxMode, Reason> {
    match raw.and_then(|cache| cache.mbx) {
        None => Ok(MbxMode::Default),
        Some(true) => Ok(MbxMode::Required),
        Some(false) if remote != MbxRemote::Off => Err(Reason::MbxOffConflict("cache.mbx_remote")),
        Some(false) if store == MbxStore::Shared => Err(Reason::MbxOffConflict("mbx_shared_store")),
        Some(false) => Ok(MbxMode::Off),
    }
}

pub(crate) fn private_sources(
    view: &RepoView,
    roots: &[RustRoot],
    config: &RustCiConfig,
) -> Result<Option<String>, DetectError> {
    let cause = crate::private_sources::private_cause(view, config, roots);
    match (config.mbx_remote, &cause) {
        (MbxRemote::GithubActions, Some(cause)) => Err(DetectError::invalid(
            &CONFIG_PATH,
            Reason::MbxRemotePrivate(cause.clone()),
        )),
        _ => Ok(cause.map(|cause| crate::private_sources::note(&cause))),
    }
}
