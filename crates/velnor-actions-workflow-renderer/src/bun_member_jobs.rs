// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use velnor_actions_contract::{
    BUN_ASSETS_IMPORT_OPERATION, BUN_ASSETS_PREPARE_OPERATION, Job, JobTimeout, PermissionLevel,
    Permissions, Step, StepKind, StepRole,
};

use crate::verification_jobs::bun_artifact_steps::{BUN_ASSETS_STAGE_DIR, bun_assets_archive_name};
use crate::{RenderError, steps};

pub const BUN_RUNNER: &str = "ubuntu-26.04";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BunMemberPolicy {
    pub name: String,
    pub id: String,
    pub timeout: JobTimeout,
    pub steps: Vec<Step>,
    pub assets: Option<String>,
}

impl BunMemberPolicy {
    #[must_use]
    pub fn reads_registry(&self) -> bool {
        reads_registry(&self.steps)
    }

    fn validate(&self) -> Result<(), RenderError> {
        validate_bun_steps(&self.id, &self.steps).and_then(|()| {
            validate_bun_consumer(&self.id, &self.id, self.assets.as_deref(), &self.steps)
        })
    }
}

pub(crate) fn validate_bun_consumer(
    id: &str,
    member_id: &str,
    assets: Option<&str>,
    steps: &[Step],
) -> Result<(), RenderError> {
    let invalid = |code: &str| RenderError::InvalidWorkflow(format!("{code}:{id}"));
    if steps.iter().any(|step| {
        matches!(
            step.role,
            Some(StepRole::PrivateTokenMint | StepRole::PrivateFetch | StepRole::PrivateRevoke)
        )
    }) {
        return Err(invalid("bun_consumer_private_credentials"));
    }

    let archive_name = bun_assets_archive_name(member_id);
    let stage_path = format!("{BUN_ASSETS_STAGE_DIR}/{member_id}");
    let mut prepare = Vec::new();
    let mut imports = Vec::new();
    let mut downloads = Vec::new();
    for (index, step) in steps.iter().enumerate() {
        match &step.kind {
            StepKind::Internal { operation, env }
                if step.role == Some(StepRole::BunAssetsHandoff)
                    && operation == BUN_ASSETS_PREPARE_OPERATION =>
            {
                prepare.push((index, env, step.condition.as_ref()));
            }
            StepKind::Internal { operation, env }
                if step.role == Some(StepRole::BunAssetsHandoff)
                    && operation == BUN_ASSETS_IMPORT_OPERATION =>
            {
                imports.push((index, env, step.condition.as_ref()));
            }
            StepKind::Action { uses, with, .. }
                if uses.starts_with("actions/download-artifact@")
                    && with
                        .get("name")
                        .is_some_and(|name| name.starts_with("velnor-pw-assets-")) =>
            {
                downloads.push((index, with, step.condition.as_ref()));
            }
            _ => {}
        }
    }
    let handoff_present = !prepare.is_empty() || !imports.is_empty() || !downloads.is_empty();
    let Some(assets) = assets else {
        return if handoff_present {
            Err(invalid("bun_consumer_asset_handoff_unexpected"))
        } else {
            Ok(())
        };
    };
    if assets != format!("{member_id}--assets")
        || prepare.len() != 1
        || imports.len() != 1
        || downloads.len() != 1
    {
        return Err(invalid("bun_consumer_asset_handoff_missing"));
    }
    let (prepare_index, prepare_env, prepare_condition) = prepare[0];
    let (download_index, download_with, download_condition) = downloads[0];
    let (import_index, import_env, import_condition) = imports[0];
    let asset_id = "VELNOR_BUN_ASSET_ID";
    let pins = "VELNOR_BUN_ASSET_PINS";
    if prepare_env.get(asset_id).map(String::as_str) != Some(member_id)
        || import_env.get(asset_id).map(String::as_str) != Some(member_id)
        || prepare_env.get(pins) != import_env.get(pins)
        || prepare_env.get(pins).is_none_or(String::is_empty)
        || download_with.get("name").map(String::as_str) != Some(archive_name.as_str())
        || download_with.get("path").map(String::as_str) != Some(stage_path.as_str())
        || download_with.get("digest-mismatch").map(String::as_str) != Some("error")
        || prepare_condition.is_some()
        || download_condition.is_some()
        || import_condition.is_some()
        || !valid_bun_consumer_prefix(&steps[..prepare_index])
        || download_index != prepare_index + 1
        || import_index != download_index + 1
        || [prepare_index, download_index, import_index]
            .iter()
            .any(|index| steps[*index].parallel)
    {
        return Err(invalid("bun_consumer_asset_handoff_malformed"));
    }
    Ok(())
}

fn valid_bun_consumer_prefix(steps: &[Step]) -> bool {
    match steps {
        [] => true,
        [step] => {
            step.role == Some(StepRole::AcquireVelnor)
                && matches!(step.kind, StepKind::Shell { .. })
                && step.condition.is_none()
                && !step.parallel
        }
        _ => false,
    }
}

pub(crate) fn validate_bun_steps(id: &str, steps: &[Step]) -> Result<(), RenderError> {
    let invalid = |code: &str| RenderError::InvalidWorkflow(format!("{code}:{id}"));
    if !id.starts_with(velnor_actions_contract::config::BUN_JOB_PREFIX) {
        return Err(invalid("bun_job_id"));
    }
    if steps.is_empty() {
        return Err(invalid("bun_job_steps_empty"));
    }
    let mut seen_parallel = false;
    let mut closed = false;
    for step in steps {
        if step.parallel {
            if closed {
                return Err(invalid("bun_parallel_group_split"));
            }
            seen_parallel = true;
        } else if seen_parallel {
            closed = true;
        }
        if matches!(&step.kind, StepKind::Shell { env, .. }
            if env.contains_key(crate::matrix::bun::BUN_LEGS_ENV)
                || env.contains_key(crate::matrix::bun::BUN_MAX_PARALLEL_ENV))
        {
            return Err(invalid("bun_matrix_marker_preset"));
        }
        if matches!(step.kind, StepKind::Action { .. })
            && step.role != Some(StepRole::PrivateTokenMint)
            && !crate::verification_jobs::stack_jobs::is_family_cache_role(step.role)
            && !crate::verification_jobs::bun_artifact_steps::is_bun_artifact_step(step)
        {
            return Err(invalid("bun_job_action_step"));
        }
    }
    validate_private_block(steps, &invalid)?;
    let installs: Vec<usize> = steps
        .iter()
        .enumerate()
        .filter(|(_, step)| step.role == Some(StepRole::PlaywrightInstall))
        .map(|(index, _)| index)
        .collect();
    if installs.len() > 1 {
        return Err(invalid("bun_playwright_install_repeated"));
    }
    if let Some(&at) = installs.first()
        && steps[..at].iter().any(|step| step.parallel)
    {
        return Err(invalid("bun_playwright_install_late"));
    }
    if steps.iter().filter(|step| is_registry_phase(step)).count() > 1 {
        return Err(invalid("bun_registry_phase_repeated"));
    }
    Ok(())
}

fn validate_private_block(
    steps: &[Step],
    invalid: &dyn Fn(&str) -> RenderError,
) -> Result<(), RenderError> {
    let roles: Vec<Option<StepRole>> = steps.iter().map(|step| step.role).collect();
    let at = roles
        .iter()
        .position(|role| *role == Some(StepRole::PrivateTokenMint));
    let private = |role: &Option<StepRole>| {
        matches!(
            role,
            Some(StepRole::PrivateTokenMint | StepRole::PrivateFetch | StepRole::PrivateRevoke)
        )
    };
    let count = roles.iter().filter(|role| private(role)).count();
    let Some(at) = at else {
        return if count == 0 {
            Ok(())
        } else {
            Err(invalid("bun_job_private_block"))
        };
    };
    if count != 3
        || roles.get(at + 1) != Some(&Some(StepRole::PrivateFetch))
        || roles.get(at + 2) != Some(&Some(StepRole::PrivateRevoke))
    {
        return Err(invalid("bun_job_private_block"));
    }
    let only_staging = |role: &Option<StepRole>| *role == Some(StepRole::AcquireVelnor);
    if !roles[..at].iter().all(only_staging) {
        return Err(invalid("bun_job_private_block_late"));
    }
    Ok(())
}

fn is_registry_phase(step: &Step) -> bool {
    step.role == Some(StepRole::BunRegistryInstall)
}

pub(crate) fn reads_registry(steps: &[Step]) -> bool {
    steps.iter().any(is_registry_phase)
}

pub(crate) fn bun_permissions(steps: &[Step]) -> Permissions {
    Permissions {
        contents: PermissionLevel::Read,
        pull_requests: PermissionLevel::None,
        id_token: PermissionLevel::None,
        actions: PermissionLevel::None,
        packages: if reads_registry(steps) {
            PermissionLevel::Read
        } else {
            PermissionLevel::None
        },
    }
}

pub fn build_bun_member_job(
    policy: &BunMemberPolicy,
    checkout_uses: &str,
) -> Result<Job, RenderError> {
    policy.validate()?;
    let mut all = vec![steps::checkout_step(checkout_uses)?];
    all.extend(policy.steps.iter().cloned());
    let mut needs = vec![crate::render::PLAN_JOB_ID.to_owned()];
    needs.extend(policy.assets.iter().cloned());
    Ok(Job {
        services: Vec::new(),
        display_name: format!("Bun / {}", policy.name),
        runs_on: BUN_RUNNER.to_owned(),
        timeout_minutes: policy.timeout,
        needs,
        condition: Some(crate::plan_gate::not_affected_gate(&policy.id)),
        permissions: Some(bun_permissions(&policy.steps)),
        cache_mode: None,
        environment: None,
        steps: all,
        check_runner: None,
    })
}

pub(crate) fn validate_bun_member_job(
    jobs: &BTreeMap<String, Job>,
    policy: &BunMemberPolicy,
    checkout_uses: &str,
    plan: Option<&velnor_actions_contract::BunLanePlan>,
) -> Result<Vec<String>, RenderError> {
    let projected = crate::verification_jobs::bun_lane_validation::project_job(
        &policy.id,
        &build_bun_member_job(policy, checkout_uses)?,
        plan,
    )?;
    for (id, expected) in &projected {
        let actual = jobs
            .get(id)
            .ok_or_else(|| RenderError::InvalidWorkflow(format!("bun_member_job_missing:{id}")))?;
        if !same_bun_job(actual, expected) {
            return Err(RenderError::InvalidWorkflow(format!(
                "bun_member_job_contract:{id}"
            )));
        }
    }
    Ok(projected.into_iter().map(|(id, _)| id).collect())
}

pub(crate) fn same_bun_job(actual: &Job, expected: &Job) -> bool {
    actual.display_name == expected.display_name
        && actual.runs_on == expected.runs_on
        && actual.timeout_minutes == expected.timeout_minutes
        && actual.needs == expected.needs
        && actual.condition == expected.condition
        && actual.permissions == expected.permissions
        && actual.environment == expected.environment
        && actual.steps == expected.steps
}
