// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;

use super::super::ValidatorKind;
use super::super::jobs::REQUIRED_GATE_JOB_ID;
use crate::config::{EPHEMERAL_CHECK_ADMISSION_CONDITION, ExecutionConfig, VelnorConfig};

/// Control jobs the renderer adds after lane expansion, so they are not part
/// of the workflow IR. They are still valid `execution.overrides` keys.
#[must_use]
pub fn is_renderer_added_control_job(job_id: &str) -> bool {
    job_id == REQUIRED_GATE_JOB_ID
        || [
            ValidatorKind::Alint,
            ValidatorKind::CargoDeny,
            ValidatorKind::CargoMachete,
            ValidatorKind::Markdownlint,
            ValidatorKind::StrictJson,
            ValidatorKind::FrontmatterId,
            ValidatorKind::LinkCheck,
            ValidatorKind::NativeValidators,
        ]
        .iter()
        .any(|kind| kind.job_id() == job_id)
}

/// Renderer-added control jobs that `execution.overrides` places on the scale set.
#[must_use]
pub fn scale_set_support_jobs(config: &VelnorConfig) -> BTreeSet<String> {
    let Some(execution) = config.execution.as_ref() else {
        return BTreeSet::new();
    };
    scale_set_support_jobs_of(execution)
}

pub(super) fn scale_set_support_jobs_of(execution: &ExecutionConfig) -> BTreeSet<String> {
    execution
        .overrides
        .iter()
        .filter(|(id, over)| {
            is_renderer_added_control_job(id) && over.profile == execution.scale_set_profile
        })
        .map(|(id, _)| id.clone())
        .collect()
}

/// The fork fence for a control job moved to the scale set: the job runs only
/// for push, merge_group and same-repository pull requests, exactly like the
/// ephemeral `check-*` jobs. A fork pull request skips it.
#[must_use]
pub fn fork_fenced_condition(original: Option<&str>) -> String {
    let admission = EPHEMERAL_CHECK_ADMISSION_CONDITION;
    let Some(original) = original else {
        return admission.to_owned();
    };
    let inner = original
        .strip_prefix("${{")
        .and_then(|rest| rest.strip_suffix("}}"))
        .map_or(original, str::trim);
    if inner == admission || inner.ends_with(&format!(") && ({admission})")) {
        return inner.to_owned();
    }
    format!("({inner}) && ({admission})")
}
