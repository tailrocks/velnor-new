//! Detect typed Terraform provider-cache restoration in lane jobs.

use std::collections::BTreeMap;

use velnor_actions_contract::{Job, StepRole};

pub(super) fn has_tofu_provider_restore(jobs: &BTreeMap<String, Job>) -> bool {
    jobs.values().any(|job| {
        job.steps
            .iter()
            .any(|step| step.role == Some(StepRole::TofuProvidersRestore))
    })
}
