//! Add the generated tar adapters before cache action steps.

use std::collections::BTreeMap;

use velnor_actions_contract::{Job, Step, StepKind, StepRole};

use crate::{RenderError, steps};

const CACHE_ACTION_ROLES: [StepRole; 6] = [
    StepRole::ToolsCacheRestore,
    StepRole::ToolsCacheSave,
    StepRole::CargoSourcesRestore,
    StepRole::CargoSourcesSave,
    StepRole::TofuProvidersRestore,
    StepRole::TofuProvidersSave,
];

/// Drop archive-cache steps from the local-build bootstrap plan.
///
/// That job creates the verified helper after source and MBX setup. It cannot
/// install the tar adapter before its earlier cache actions.
pub(crate) fn strip_preseed_bootstrap_cache(job: &mut Job) {
    job.steps
        .retain(|step| !is_preseed_bootstrap_cache_step(step.role));
}

fn is_preseed_bootstrap_cache_step(role: Option<StepRole>) -> bool {
    matches!(
        role,
        Some(
            StepRole::ToolsCacheIdentity
                | StepRole::ToolSeed
                | StepRole::ToolsCacheRestore
                | StepRole::ToolsCacheSave
                | StepRole::NormalToolsCacheStage
                | StepRole::NormalToolsCacheImport
                | StepRole::NormalToolsCacheExport
                | StepRole::CargoSourcesRestore
                | StepRole::CargoSourcesSave
                | StepRole::TofuProvidersRestore
                | StepRole::TofuProvidersSave
        )
    )
}

/// Add one installer after helper staging and before the first archive action.
///
/// # Errors
///
/// Gives an error when a typed cache action lacks one valid staged helper or
/// cannot run after checkout and before any internal operation.
pub(crate) fn insert_launcher_steps(jobs: &mut BTreeMap<String, Job>) -> Result<(), RenderError> {
    for (job_id, job) in jobs {
        insert_job_launcher(job_id, job)?;
    }
    Ok(())
}

fn insert_job_launcher(job_id: &str, job: &mut Job) -> Result<(), RenderError> {
    let Some(cache_at) = job.steps.iter().position(is_cache_action) else {
        return Ok(());
    };
    let staged = job
        .steps
        .iter()
        .enumerate()
        .filter(|(_, step)| is_helper_stage(step))
        .map(|(index, _)| index)
        .collect::<Vec<_>>();
    if staged.len() != 1 {
        return Err(RenderError::InvalidWorkflow(format!(
            "cache_action_helper_stage_count:{}:{}",
            job_id,
            staged.len()
        )));
    }
    let helper_sha256 = helper_sha256(&job.steps[staged[0]], job_id)?;
    let checkout_end = job
        .steps
        .iter()
        .rposition(|step| step.role == Some(StepRole::Checkout))
        .map_or(0, |index| index + 1);
    if staged[0] < checkout_end {
        return Err(RenderError::InvalidWorkflow(format!(
            "cache_helper_before_checkout:{job_id}"
        )));
    }
    if staged[0] >= cache_at {
        return Err(RenderError::InvalidWorkflow(format!(
            "cache_action_before_helper:{job_id}"
        )));
    }
    let internal_at = job
        .steps
        .iter()
        .position(|step| matches!(step.kind, StepKind::Internal { .. }) && !is_launcher(step));
    let boundary = internal_at.map_or(cache_at, |index| index.min(cache_at));
    if boundary <= staged[0] {
        return Err(RenderError::InvalidWorkflow(format!(
            "cache_action_before_helper_or_internal:{job_id}"
        )));
    }

    ensure_one_launcher(job_id, job, &helper_sha256)?;
    validate_action_local_path(job_id, job)?;
    Ok(())
}

fn helper_sha256(step: &Step, job_id: &str) -> Result<String, RenderError> {
    let digest = match step.role {
        Some(StepRole::AcquireVelnor) => match &step.kind {
            StepKind::Shell { env, .. } => env.get(steps::ASSET_SHA_ENV).cloned(),
            _ => None,
        },
        Some(StepRole::PreseedStage)
            if step.id == Some(velnor_actions_contract::StepId::PreseedStage) =>
        {
            Some(steps::PRESEED_HELPER_SHA256_OUTPUT_EXPR.to_owned())
        }
        _ => None,
    };
    digest
        .filter(|value| {
            velnor_actions_contract::ids::is_lower_hex_len(value, 64)
                || value == steps::PRESEED_HELPER_SHA256_OUTPUT_EXPR
        })
        .ok_or_else(|| {
            RenderError::InvalidWorkflow(format!("cache_action_helper_digest_missing:{job_id}"))
        })
}

fn ensure_one_launcher(
    job_id: &str,
    job: &mut Job,
    helper_sha256: &str,
) -> Result<(), RenderError> {
    let positions = job
        .steps
        .iter()
        .enumerate()
        .filter(|(_, step)| is_launcher(step))
        .map(|(index, _)| index)
        .collect::<Vec<_>>();
    if positions.len() > 1 {
        return Err(RenderError::InvalidWorkflow(format!(
            "duplicate_cache_tar_launcher:{job_id}"
        )));
    }
    let helper_at = job
        .steps
        .iter()
        .position(is_helper_stage)
        .ok_or_else(|| RenderError::InvalidWorkflow("cache_helper_stage_missing".to_owned()))?;
    if let Some(position) = positions.first().copied() {
        if !valid_launcher_step(&job.steps[position], helper_sha256) {
            return Err(RenderError::InvalidWorkflow(format!(
                "cache_tar_launcher_malformed:{job_id}"
            )));
        }
        if position != helper_at + 1 {
            return Err(RenderError::InvalidWorkflow(format!(
                "cache_tar_launcher_order:{job_id}"
            )));
        }
        return Ok(());
    }
    job.steps.insert(
        helper_at + 1,
        steps::cache_tar_launcher_step(helper_sha256)?,
    );
    Ok(())
}

fn validate_action_local_path(job_id: &str, job: &Job) -> Result<(), RenderError> {
    for step in &job.steps {
        if !requires_cache_tar_path(step.role) {
            continue;
        }
        let StepKind::Action { env, .. } = &step.kind else {
            return Err(RenderError::InvalidWorkflow(format!(
                "cache_action_not_an_action:{job_id}:{}",
                step.name
            )));
        };
        if env
            .get("PATH")
            .is_some_and(|path| path != steps::CACHE_TAR_LAUNCHER_PATH_OUTPUT_EXPR)
        {
            return Err(RenderError::InvalidWorkflow(format!(
                "cache_action_path_override:{job_id}:{}",
                step.name
            )));
        }
    }
    Ok(())
}

pub(crate) fn requires_cache_tar_path(role: Option<StepRole>) -> bool {
    role.is_some_and(|role| CACHE_ACTION_ROLES.contains(&role))
}

fn is_helper_stage(step: &Step) -> bool {
    matches!(
        step.role,
        Some(StepRole::AcquireVelnor | StepRole::PreseedStage)
    )
}

fn is_cache_action(step: &Step) -> bool {
    step.role
        .is_some_and(|role| CACHE_ACTION_ROLES.contains(&role))
}

fn is_launcher(step: &Step) -> bool {
    step.role == Some(StepRole::CacheTarLaunchers)
}

fn valid_launcher_step(step: &Step, helper_sha256: &str) -> bool {
    step.name == "Install cache tar adapters"
        && step.id == Some(velnor_actions_contract::StepId::CacheTarLaunchers)
        && step.role == Some(StepRole::CacheTarLaunchers)
        && step.condition.is_none()
        && matches!(
            &step.kind,
            StepKind::Action { uses, with, env }
                if uses == steps::CACHE_TAR_LAUNCHER_USES
                    && env.is_empty()
                    && with == &BTreeMap::from([(
                        steps::CACHE_TAR_LAUNCHER_SHA256_INPUT.to_owned(),
                        helper_sha256.to_owned(),
                    )])
        )
}

#[cfg(test)]
#[path = "cache_tar_setup_tests.rs"]
mod tests;
