// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde::{Deserialize, Serialize};

use crate::journal::pr_source_transition::model::validation::{valid_ref, valid_sha};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ReferenceReceipt {
    pub(crate) reference: String,
    pub(crate) object_type: String,
    pub(crate) sha: String,
}

impl ReferenceReceipt {
    pub(crate) fn is_well_formed(&self) -> bool {
        self.reference.len() <= 512
            && valid_ref(
                self.reference
                    .strip_prefix("refs/heads/")
                    .unwrap_or_default(),
            )
            && !self.object_type.is_empty()
            && self.object_type.len() <= 64
            && self
                .object_type
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric())
            && valid_sha(&self.sha)
    }
}
