// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde::Deserialize;
use velnor_model::PushAfterMergeGroup;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RawCi {
    skip_push_after_merge_group: Option<bool>,
}

pub(crate) fn push_after_merge_group(ci: Option<RawCi>) -> PushAfterMergeGroup {
    ci.and_then(|ci| ci.skip_push_after_merge_group)
        .unwrap_or(false)
        .into()
}
