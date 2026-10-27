// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};

use velnor_actions_contract::{BunLanePlan, Job, config::BUN_JOB_PREFIX};

use crate::{RenderError, verification_jobs::WorkflowTaskPolicy};

pub(crate) fn project_job(
    logical_id: &str,
    canonical_job: &Job,
    plan: Option<&BunLanePlan>,
) -> Result<Vec<(String, Job)>, RenderError> {
    match plan {
        Some(plan) => plan
            .project(logical_id, canonical_job)
            .map_err(RenderError::Contract),
        None => Ok(vec![(logical_id.to_owned(), canonical_job.clone())]),
    }
}

pub(crate) fn output_ids(
    logical_id: &str,
    plan: Option<&BunLanePlan>,
) -> Result<Vec<String>, RenderError> {
    match plan {
        Some(plan) => plan.output_ids(logical_id).map_err(RenderError::Contract),
        None => Ok(vec![logical_id.to_owned()]),
    }
}

pub(crate) fn validate_output_set(
    jobs: &BTreeMap<String, Job>,
    policies: &[WorkflowTaskPolicy],
    plan: Option<&BunLanePlan>,
) -> Result<(), RenderError> {
    let Some(plan) = plan else {
        return Ok(());
    };
    let mut expected = BTreeSet::new();
    for policy in policies {
        for logical_id in bun_logical_ids(policy) {
            expected.extend(output_ids(&logical_id, Some(plan))?);
        }
    }
    let actual: BTreeSet<String> = jobs
        .keys()
        .filter(|id| id.starts_with(BUN_JOB_PREFIX))
        .cloned()
        .collect();
    if actual != expected {
        return Err(RenderError::InvalidWorkflow(
            "bun_job_output_set_mismatch".to_owned(),
        ));
    }
    Ok(())
}

fn bun_logical_ids(policy: &WorkflowTaskPolicy) -> Vec<String> {
    match policy {
        WorkflowTaskPolicy::Bun(policy) => vec![policy.id.clone()],
        WorkflowTaskPolicy::BunSuite(policy) => policy.job_ids(),
        WorkflowTaskPolicy::BunAssets(policy) => vec![policy.id.clone()],
        _ => Vec::new(),
    }
}
