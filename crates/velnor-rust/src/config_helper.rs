// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde::Deserialize;
use velnor_model::RepoPath;

use crate::error::Reason;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RawHelper {
    pub(crate) path: String,
}

pub(crate) fn path(helper: Option<RawHelper>) -> Result<Option<RepoPath>, Reason> {
    helper
        .map(|helper| {
            RepoPath::new(helper.path.trim_end_matches('/')).map_err(|_| Reason::InvalidValue {
                key: "helper.path",
                value: helper.path,
            })
        })
        .transpose()
}
