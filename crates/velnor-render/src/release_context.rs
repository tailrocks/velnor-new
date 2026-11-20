// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_model::{HelperRelease, PrivateSources};
use velnor_release::BinaryPlan;

use crate::rules::RenderedStep;

pub(crate) struct Private<'a> {
    pub(crate) helper: &'a HelperRelease,
    pub(crate) sources: &'a PrivateSources,
}

pub(crate) struct BuildContext<'a> {
    pub(crate) source: &'a str,
    pub(crate) same_commit: RenderedStep,
    pub(crate) tag: &'a str,
    pub(crate) private: Option<Private<'a>>,
    pub(crate) mbx: bool,
}

pub(crate) fn package_tag(binary: &BinaryPlan) -> String {
    format!("${{{{ needs.tag.outputs.{} }}}}", binary.package)
}

/// Release builds always use a fresh private mbx store; never shared, never restored.
///
/// mbx's action records are unsigned, so a store that another job, event or
/// restored cache could write would let that writer choose what a release
/// build serves. The step points `MBX_CACHE_DIR` at a job-named directory under
/// `$RUNNER_TEMP`, which the runner empties for every job, before the tools
/// install or any cargo runs.
pub(crate) fn private_mbx_store(job: &str) -> RenderedStep {
    RenderedStep {
        name: "Private mbx store".to_owned(),
        run: Some(format!(
            "echo \"MBX_CACHE_DIR=$RUNNER_TEMP/mbx-release-{job}\" >> \"$GITHUB_ENV\""
        )),
        ..RenderedStep::default()
    }
}
