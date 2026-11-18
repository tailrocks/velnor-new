// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_actions_contract::workflow::crate_job::{
    CrateGroupPlan, group_timeout_for, validate_group_needs,
};
use velnor_actions_contract::{CrateJob, JobTimeout};

use crate::OrchestratorError;

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
