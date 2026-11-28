// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde::Deserialize;
use velnor_model::{PushAfterMergeGroup, ReusePrVerdict};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RawCi {
    skip_push_after_merge_group: Option<bool>,
    reuse_pr_verdict: Option<bool>,
}

pub(crate) fn push_after_merge_group(ci: Option<RawCi>) -> PushAfterMergeGroup {
    ci.and_then(|ci| ci.skip_push_after_merge_group)
        .unwrap_or(false)
        .into()
}

pub(crate) fn reuse_pr_verdict(ci: Option<&RawCi>) -> ReusePrVerdict {
    ci.and_then(|ci| ci.reuse_pr_verdict)
        .unwrap_or(false)
        .into()
}
