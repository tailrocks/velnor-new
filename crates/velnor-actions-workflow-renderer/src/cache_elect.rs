//! Per-key tools-cache writer election.
//!
//! Exactly one saver per V2 runtime-qualified key, elected after every
//! restore is inserted. Split from `cache_p08` (size gate).

use std::collections::BTreeMap;

use velnor_actions_contract::{Job, Step, StepKind, StepRole};

use crate::RenderError;

/// Elect one V2 tools-cache writer per runtime-qualified key across jobs.
///
/// Exactly one job per key receives a push-gated save. The plan job wins
/// when it shares the key; otherwise the lowest job ID is deterministic.
///
/// # Errors
///
/// Returns [`RenderError`] when a winner's save step fails to build
/// (unreachable for keys read back from valid setups).
pub fn elect_tools_cache_writers(jobs: &mut BTreeMap<String, Job>) -> Result<(), RenderError> {
    let mut by_identity: BTreeMap<(String, String), Vec<String>> = BTreeMap::new();
    for (id, job) in jobs.iter() {
        if let Some(identity) = tools_restore_identity(id, job)? {
            by_identity.entry(identity).or_default().push(id.clone());
        }
    }
    for ((key, _static_digest), owners) in &by_identity {
        let winner = owners
            .iter()
            .find(|id| id.as_str() == crate::render::PLAN_JOB_ID)
            .or_else(|| owners.iter().min())
            .map(String::as_str)
            .unwrap_or_default();
        if let Some(job) = jobs.get_mut(winner) {
            append_tools_save(job, key)?;
        }
    }
    Ok(())
}

/// Elect one provider-cache writer per key across jobs.
///
/// Every tofu job restores its own root key read-only; exactly one
/// job per key gets a push-gated `Save Tofu providers` step over
/// that key. Keys are per-root so the winner is usually the sole
/// owner; a shared key (one root under another configuration) goes
/// to the lowest job ID, so the election stays deterministic across
/// renders. The plan job never restores providers, so it never wins.
/// Reruns add no second save to a job that already carries one.
///
/// # Errors
///
/// Returns [`RenderError`] when a winner's save step fails to build
/// (unreachable for keys read back from valid restores).
pub fn elect_tofu_provider_savers(jobs: &mut BTreeMap<String, Job>) -> Result<(), RenderError> {
    let mut by_key: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut path_for: BTreeMap<String, String> = BTreeMap::new();
    for (id, job) in jobs.iter() {
        velnor_actions_contract::workflow::step_identity::validate_step_sequence(&job.steps, id)
            .map_err(RenderError::Contract)?;
        if let Some((key, path)) = provider_restore_entry(job) {
            if path_for.get(&key).is_some_and(|existing| existing != &path) {
                return Err(RenderError::InvalidWorkflow(format!(
                    "tofu_cache_key_path_mismatch:{id}"
                )));
            }
            path_for.entry(key.clone()).or_insert(path);
            by_key.entry(key).or_default().push(id.clone());
        }
    }
    for (key, owners) in &by_key {
        let Some(winner) = owners.iter().min() else {
            continue;
        };
        let Some(path) = path_for.get(key) else {
            continue;
        };
        for owner in owners {
            if let Some(job) = jobs.get_mut(owner) {
                if owner == winner {
                    append_provider_save(job, key, path)?;
                } else if has_provider_save(job) {
                    return Err(RenderError::InvalidWorkflow(format!(
                        "tofu_provider_save_not_elected:{owner}"
                    )));
                }
            }
        }
    }
    Ok(())
}

/// This job's provider-restore `(key, path)`, when it restores one.
///
/// Structural match only: the restore action plus exactly one owned
/// plugin-cache path. Names stay out: a display rename must never
/// move an election.
fn provider_restore_entry(job: &Job) -> Option<(String, String)> {
    job.steps.iter().find_map(|step| {
        if step.role != Some(StepRole::TofuProvidersRestore) {
            return None;
        }
        let StepKind::Action { uses, with, .. } = &step.kind else {
            return None;
        };
        if uses != velnor_actions_contract::workflow::step_identity::TOFU_PROVIDER_ADMISSION_USES {
            return None;
        }
        let key = with.get("cache-key")?;
        if key.trim().is_empty() {
            return None;
        }
        let path = with.get("cache-path")?;
        if !crate::tofu_cache::tofu_providers_path_ok(path) {
            return None;
        }
        Some((key.clone(), path.clone()))
    })
}

/// Append the push-gated provider save over `key` to one writer job.
///
/// Mirrors `Save Cargo sources`: the trusted save gate keeps PR runs
/// read-only, and the step archives the exact entry the restore
/// reads, so a push-seeded entry warms every later restore of the
/// key. Jobs that already carry the save keep exactly one.
fn append_provider_save(job: &mut Job, key: &str, path: &str) -> Result<(), RenderError> {
    if provider_restore_entry(job)
        .as_ref()
        .is_none_or(|(restore_key, restore_path)| restore_key != key || restore_path != path)
    {
        return Err(RenderError::InvalidWorkflow(
            "tofu_provider_save_restore_mismatch".to_owned(),
        ));
    }
    let saves: Vec<&Step> = job
        .steps
        .iter()
        .filter(|step| step.role == Some(StepRole::TofuProvidersSave))
        .collect();
    if saves.len() > 1 {
        return Err(RenderError::InvalidWorkflow(
            "tofu_provider_save_duplicate".to_owned(),
        ));
    }
    if let Some(save) = saves.first() {
        let StepKind::Action { uses, with, env } = &save.kind else {
            return Err(RenderError::InvalidWorkflow(
                "tofu_provider_save_shape".to_owned(),
            ));
        };
        if !uses.starts_with("actions/cache/save@")
            || !env.is_empty()
            || with.len() != 2
            || with.get("key").map(String::as_str)
                != Some(velnor_actions_contract::workflow::step_identity::TOFU_PROVIDERS_KEY_OUTPUT_EXPR)
            || with.get("path").map(String::as_str)
                != Some(velnor_actions_contract::workflow::step_identity::TOFU_PROVIDERS_PATH_OUTPUT_EXPR)
        {
            return Err(RenderError::InvalidWorkflow(
                "tofu_provider_save_restore_mismatch".to_owned(),
            ));
        }
        if save.condition.as_deref()
            != Some(velnor_actions_contract::workflow::ir::CACHE_SAVE_CONDITION)
        {
            return Err(RenderError::InvalidWorkflow(
                "tofu_provider_save_gate_mismatch".to_owned(),
            ));
        }
        return Ok(());
    }
    let mut save = crate::tofu_cache::tofu_providers_save_step()?;
    save.condition = Some(velnor_actions_contract::workflow::ir::CACHE_SAVE_CONDITION.to_owned());
    job.steps.push(save);
    Ok(())
}

/// True when one job already carries a typed provider save.
fn has_provider_save(job: &Job) -> bool {
    job.steps
        .iter()
        .any(|step| step.role == Some(StepRole::TofuProvidersSave))
}

/// This job's V2 tools key after validating its identity-gated restore.
fn tools_restore_identity(id: &str, job: &Job) -> Result<Option<(String, String)>, RenderError> {
    let restores = job
        .steps
        .iter()
        .filter(|step| step.role == Some(StepRole::ToolsCacheRestore))
        .collect::<Vec<_>>();
    if restores.is_empty() {
        return Ok(None);
    }
    let identities = job
        .steps
        .iter()
        .filter(|step| step.role == Some(StepRole::ToolsCacheIdentity))
        .collect::<Vec<_>>();
    if restores.len() != 1 || identities.len() != 1 {
        return Err(RenderError::InvalidWorkflow(format!(
            "tools_cache_restore_shape:{id}"
        )));
    }
    let step = restores[0];
    let key = crate::cache_steps::validate_tools_restore_call(step)
        .map(str::to_owned)
        .map_err(|_| RenderError::InvalidWorkflow(format!("tools_cache_restore_shape:{id}")))?;
    let identity = identities[0];
    let StepKind::Action {
        uses,
        with: identity_with,
        env: identity_env,
    } = &identity.kind
    else {
        return Err(RenderError::InvalidWorkflow(format!(
            "tools_cache_identity_shape:{id}"
        )));
    };
    if identity.id.is_some() || identity.condition.is_some() {
        return Err(RenderError::InvalidWorkflow(format!(
            "tools_cache_identity_shape:{id}"
        )));
    }
    crate::cache_p08::validate_runtime_identity_action(
        identity,
        uses,
        &job.runs_on,
        identity_with,
        identity_env,
    )
    .map_err(|_| RenderError::InvalidWorkflow(format!("tools_cache_identity_shape:{id}")))?;
    let static_digest = identity_with
        .get(velnor_actions_contract::workflow::step_identity::TOOLS_CACHE_IDENTITY_DIGEST_INPUT)
        .cloned()
        .ok_or_else(|| {
            RenderError::InvalidWorkflow(format!("tools_cache_identity_digest_missing:{id}"))
        })?;
    Ok(Some((key, static_digest)))
}

/// Append the push-gated tools save over `key` to one writer job.
///
/// Mirrors `Save Cargo sources`: the trusted save gate keeps PR runs
/// read-only, and the step archives the exact tool payload the V2
/// restore reads, so a push-seeded entry warms every later
/// restore of the key. Closures and fan-in steps added after the
/// election install no tools, so the capture stays complete.
fn append_tools_save(job: &mut Job, key: &str) -> Result<(), RenderError> {
    let mut expected = crate::steps::tools_cache_step(
        false,
        key,
        Some(crate::cache_p08::tools_cache_save_condition()),
    )?;
    expected.role = Some(StepRole::ToolsCacheSave);
    let existing = job
        .steps
        .iter()
        .filter(|step| step.role == Some(StepRole::ToolsCacheSave))
        .collect::<Vec<_>>();
    if existing.len() > 1 || existing.first().is_some_and(|step| *step != &expected) {
        return Err(RenderError::InvalidWorkflow(
            "tools_cache_save_shape".to_owned(),
        ));
    }
    if existing.is_empty() {
        job.steps.push(expected);
    }
    Ok(())
}
