// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde::Deserialize;
use velnor_model::MbxRemote;

use crate::error::Reason;

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
