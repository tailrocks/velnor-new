// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use velnor_actions_contract::{
    BUN_ASSETS_EXPORT_OPERATION, Job, JobTimeout, Step, StepKind, StepRole,
};

use crate::verification_jobs::bun_artifact_steps::{BUN_ASSETS_STAGE_DIR, bun_assets_archive_name};
use crate::verification_jobs::bun_member_jobs::{
    BUN_RUNNER, bun_permissions, same_bun_job, validate_bun_steps,
};
use crate::{RenderError, steps};

pub const BUN_ASSETS_SUFFIX: &str = "--assets";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BunAssetsPolicy {
    pub member: String,
    pub id: String,
    pub timeout: JobTimeout,
    pub steps: Vec<Step>,
}

impl BunAssetsPolicy {
    fn validate(&self) -> Result<(), RenderError> {
        let invalid = |code: &str| RenderError::InvalidWorkflow(format!("{code}:{}", self.id));
        if !self.id.ends_with(BUN_ASSETS_SUFFIX) {
            return Err(invalid("bun_assets_job_id"));
        }
        validate_bun_steps(&self.id, &self.steps)?;
        if !self
            .steps
            .iter()
            .any(|step| step.role == Some(StepRole::PrivateFetch))
        {
            return Err(invalid("bun_assets_without_fetch"));
        }
        validate_asset_producer(&self.id, &self.steps)?;
        if self.steps.iter().any(|step| step.parallel) {
            return Err(invalid("bun_assets_parallel_step"));
        }
        Ok(())
    }
}

fn validate_asset_producer(id: &str, steps: &[Step]) -> Result<(), RenderError> {
    let invalid = |code: &str| RenderError::InvalidWorkflow(format!("{code}:{id}"));
    let member_id = id
        .strip_suffix(BUN_ASSETS_SUFFIX)
        .ok_or_else(|| invalid("bun_assets_job_id"))?;
    let archive = bun_assets_archive_name(member_id);
    let archive_path = format!("{BUN_ASSETS_STAGE_DIR}/{member_id}/{archive}");
    let mut exports = Vec::new();
    let mut uploads = Vec::new();
    let mut revokes = Vec::new();
    let mut acquires = Vec::new();
    for (index, step) in steps.iter().enumerate() {
        let accepted = match (&step.kind, step.role) {
            (StepKind::Shell { .. }, Some(StepRole::AcquireVelnor)) => {
                acquires.push(index);
                true
            }
            (StepKind::Action { uses, .. }, Some(StepRole::PrivateTokenMint)) => {
                uses.starts_with("actions/create-github-app-token@")
            }
            (StepKind::Internal { .. }, Some(StepRole::PrivateFetch | StepRole::PrivateRevoke)) => {
                if step.role == Some(StepRole::PrivateRevoke) {
                    revokes.push(index);
                }
                true
            }
            (StepKind::Internal { operation, env }, Some(StepRole::BunAssetsHandoff))
                if operation == BUN_ASSETS_EXPORT_OPERATION
                    && env.get("VELNOR_BUN_ASSET_ID").map(String::as_str) == Some(member_id)
                    && env
                        .get("VELNOR_BUN_ASSET_PINS")
                        .is_some_and(|pins| !pins.is_empty()) =>
            {
                exports.push(index);
                true
            }
            (StepKind::Action { uses, with, .. }, None)
                if uses.starts_with("actions/upload-artifact@")
                    && with.get("path").map(String::as_str) == Some(archive_path.as_str())
                    && with.get("archive").map(String::as_str) == Some("false")
                    && with.get("if-no-files-found").map(String::as_str) == Some("error") =>
            {
                uploads.push(index);
                true
            }
            _ => false,
        };
        if !accepted {
            return Err(invalid("bun_assets_producer_untrusted_step"));
        }
    }
    if acquires.as_slice() != [0]
        || exports.len() != 1
        || uploads.len() != 1
        || revokes.len() != 1
        || exports[0] != revokes[0] + 1
        || uploads[0] != exports[0] + 1
    {
        return Err(invalid("bun_assets_handoff_malformed"));
    }
    Ok(())
}

pub fn build_bun_assets_job(
    policy: &BunAssetsPolicy,
    checkout_uses: &str,
) -> Result<Job, RenderError> {
    policy.validate()?;
    let mut all = vec![steps::checkout_step(checkout_uses)?];
    all.extend(policy.steps.iter().cloned());
    Ok(Job {
        services: Vec::new(),
        outputs: BTreeMap::new(),
        display_name: format!("Bun / {} / assets", policy.member),
        runs_on: BUN_RUNNER.to_owned(),
        timeout_minutes: policy.timeout,
        needs: vec![crate::render::PLAN_JOB_ID.to_owned()],
        condition: Some(crate::plan_gate::not_affected_gate(&policy.id)),
        permissions: Some(bun_permissions(&policy.steps)),
        cache_mode: None,
        environment: None,
        steps: all,
        check_runner: None,
    })
}

pub(crate) fn validate_bun_assets_job(
    jobs: &BTreeMap<String, Job>,
    policy: &BunAssetsPolicy,
    checkout_uses: &str,
    plan: Option<&velnor_actions_contract::BunLanePlan>,
) -> Result<Vec<String>, RenderError> {
    let expected = crate::verification_jobs::bun_lane_validation::project_job(
        &policy.id,
        &build_bun_assets_job(policy, checkout_uses)?,
        plan,
    )?;
    for (id, expected_job) in &expected {
        let actual = jobs
            .get(id)
            .ok_or_else(|| RenderError::InvalidWorkflow(format!("bun_assets_job_missing:{id}")))?;
        if !same_bun_job(actual, expected_job) {
            return Err(RenderError::InvalidWorkflow(format!(
                "bun_assets_job_contract:{id}"
            )));
        }
    }
    Ok(Vec::new())
}
