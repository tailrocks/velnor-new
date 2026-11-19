// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use velnor_actions_contract::workflow::crate_job::{
    CrateGroupPlan, group_timeout_for, validate_group_needs,
};
use velnor_actions_contract::{CrateJob, JobTimeout, RustCrateConfig, Step, StepKind};

use super::is_rust;
use crate::OrchestratorError;
use crate::discover::Discovery;

pub(super) fn group_plan_for(
    model: &CrateJob,
    workspace_root: &str,
    environment: &str,
    bound: JobTimeout,
) -> Result<CrateGroupPlan, OrchestratorError> {
    let mut unit_ids: Vec<String> = model
        .obligations
        .iter()
        .map(|obligation| obligation.task_id.clone())
        .collect();
    unit_ids.sort();
    Ok(CrateGroupPlan::new(
        model.job_id.clone(),
        workspace_root.to_owned(),
        environment.to_owned(),
        unit_ids,
        group_timeout_for(None, bound),
    )?)
}

pub(super) fn reject_seed_needs(needs: &[String], job_id: &str) -> Result<(), OrchestratorError> {
    Ok(validate_group_needs(needs, job_id)?)
}

pub(super) fn reject_unknown_crates(
    crates: &BTreeMap<String, RustCrateConfig>,
    discovery: &Discovery,
) -> Result<(), OrchestratorError> {
    for package in crates.keys() {
        if !discovery
            .proposals
            .iter()
            .any(|task| task.display_name == *package && is_rust(task))
        {
            return Err(OrchestratorError::config(
                crate::config::CONFIG_REL,
                format!("stacks.rust.crates.{package}"),
                "unknown_rust_package",
            ));
        }
    }
    Ok(())
}

const CARGO_BUILD_JOBS_ENV: &str = "CARGO_BUILD_JOBS";

pub(super) fn cap_cargo_jobs(step: &mut Step, build_jobs: u32) {
    if let StepKind::TaskExecution { env, .. } = &mut step.kind {
        env.insert(CARGO_BUILD_JOBS_ENV.to_owned(), build_jobs.to_string());
    }
}
