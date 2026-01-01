//! Per-key cache writer election.
//!
//! Exactly one saver per V2 runtime-qualified key, elected after every
//! restore is inserted. Split from `cache_p08` (size gate).

use std::collections::BTreeMap;

use velnor_actions_contract::{Job, Step, StepKind, StepRole};

use crate::RenderError;

/// Validate both cache families, then append every elected save atomically.
///
/// Exactly one job per key receives a push-gated save. The plan job wins
/// when it shares the key; otherwise the lowest job ID is deterministic.
///
/// # Errors
///
/// Returns [`RenderError`] when a restore or existing save is malformed,
/// when a save is orphaned or belongs to a non-elected job, or when a
/// winner's save step cannot be built.
pub fn elect_cache_writers(jobs: &mut BTreeMap<String, Job>) -> Result<(), RenderError> {
    let mut planned = Vec::new();
    planned.extend(plan_tools_saves(jobs)?);
    planned.extend(plan_provider_saves(jobs)?);
    for (id, save) in planned {
        let Some(job) = jobs.get_mut(&id) else {
            return Err(RenderError::InvalidWorkflow(
                "cache_writer_missing".to_owned(),
            ));
        };
        job.steps.push(save);
    }
    Ok(())
}

/// Construct, but do not append, validated V2 tools saves.
fn plan_tools_saves(jobs: &BTreeMap<String, Job>) -> Result<Vec<(String, Step)>, RenderError> {
    let mut by_identity: BTreeMap<(String, String), Vec<String>> = BTreeMap::new();
    for (id, job) in jobs {
        let identity = tools_restore_identity(id, job)?;
        if identity.is_none() && has_tools_save(job) {
            return Err(RenderError::InvalidWorkflow(format!(
                "tools_cache_save_orphan:{id}"
            )));
        }
        if let Some(identity) = identity {
            by_identity.entry(identity).or_default().push(id.clone());
        }
    }
    let mut elected = BTreeMap::new();
    for ((key, digest), owners) in &by_identity {
        let winner = owners
            .iter()
            .find(|id| id.as_str() == crate::render::PLAN_JOB_ID)
            .or_else(|| owners.iter().min())
            .map(String::as_str)
            .unwrap_or_default();
        if elected
            .insert(winner.to_owned(), (key.clone(), digest.clone()))
            .is_some()
        {
            return Err(RenderError::InvalidWorkflow(
                "tools_cache_multiple_winner_keys".to_owned(),
            ));
        }
    }
    for (id, job) in jobs {
        let saves = tools_save_steps(job);
        if let Some((key, _)) = elected.get(id) {
            validate_existing_tools_save(job, key)?;
        } else if !saves.is_empty() {
            let kind = if has_tools_restore(job) {
                "tools_cache_save_not_elected"
            } else {
                "tools_cache_save_orphan"
            };
            return Err(RenderError::InvalidWorkflow(format!("{kind}:{id}")));
        }
    }
    let mut planned = Vec::new();
    for (id, (key, _digest)) in elected {
        let Some(job) = jobs.get(&id) else {
            return Err(RenderError::InvalidWorkflow(
                "tools_cache_winner_missing".to_owned(),
            ));
        };
        if tools_save_steps(job).is_empty() {
            planned.push((id, expected_tools_save(&key)?));
        }
    }
    Ok(planned)
}

/// True when a job carries one typed V2 tools restore or identity step.
fn has_tools_restore(job: &Job) -> bool {
    job.steps
        .iter()
        .any(|step| step.role == Some(StepRole::ToolsCacheRestore))
}

/// True when a job carries a typed V2 tools save.
fn has_tools_save(job: &Job) -> bool {
    job.steps
        .iter()
        .any(|step| step.role == Some(StepRole::ToolsCacheSave))
}

fn tools_save_steps(job: &Job) -> Vec<&Step> {
    job.steps
        .iter()
        .filter(|step| step.role == Some(StepRole::ToolsCacheSave))
        .collect()
}

fn expected_tools_save(key: &str) -> Result<Step, RenderError> {
    let mut expected = crate::steps::tools_cache_step(
        false,
        key,
        Some(crate::cache_p08::tools_cache_save_condition()),
    )?;
    expected.role = Some(StepRole::ToolsCacheSave);
    Ok(expected)
}

fn validate_existing_tools_save(job: &Job, key: &str) -> Result<(), RenderError> {
    let expected = expected_tools_save(key)?;
    let saves = tools_save_steps(job);
    if saves.len() > 1
        || saves
            .first()
            .is_some_and(|step| !crate::cache_p08::same_step_semantics(step, &expected))
    {
        return Err(RenderError::InvalidWorkflow(
            "tools_cache_save_shape".to_owned(),
        ));
    }
    Ok(())
}

/// Construct, but do not append, validated `OpenTofu` provider saves.
///
/// Every `OpenTofu` job restores its own root key read-only; exactly one
/// job per key gets a push-gated `Save Tofu providers` step over
/// that key. Keys are per-root so the winner is usually the sole
/// owner; a shared key (one root under another configuration) goes
/// to the lowest job ID, so the election stays deterministic across
/// renders. The plan job never restores providers, so it never wins.
/// Reruns add no second save to a job that already carries one.
///
/// # Errors
///
/// Returns [`RenderError`] when any restore or existing save is malformed.
fn plan_provider_saves(jobs: &BTreeMap<String, Job>) -> Result<Vec<(String, Step)>, RenderError> {
    let mut by_key: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut path_for: BTreeMap<String, String> = BTreeMap::new();
    for (id, job) in jobs {
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
    let mut planned = Vec::new();
    for (key, owners) in &by_key {
        let Some(winner) = owners.iter().min() else {
            continue;
        };
        let Some(path) = path_for.get(key) else {
            return Err(RenderError::InvalidWorkflow(
                "tofu_provider_path_missing".to_owned(),
            ));
        };
        for owner in owners {
            let Some(job) = jobs.get(owner) else {
                return Err(RenderError::InvalidWorkflow(
                    "tofu_provider_owner_missing".to_owned(),
                ));
            };
            if owner != winner && has_provider_save(job) {
                return Err(RenderError::InvalidWorkflow(format!(
                    "tofu_provider_save_not_elected:{owner}"
                )));
            }
            validate_provider_save(job, key, path)?;
            if owner == winner && !has_provider_save(job) {
                let mut save = crate::tofu_cache::tofu_providers_save_step()?;
                save.condition =
                    Some(velnor_actions_contract::workflow::ir::CACHE_SAVE_CONDITION.to_owned());
                planned.push((owner.clone(), save));
            }
        }
    }
    // A typed provider save without a restore has no elected owner.
    for (id, job) in jobs {
        if has_provider_save(job) && provider_restore_entry(job).is_none() {
            return Err(RenderError::InvalidWorkflow(format!(
                "tofu_provider_save_orphan:{id}"
            )));
        }
    }
    Ok(planned)
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
        let StepKind::Action { uses, with, env } = &step.kind else {
            return None;
        };
        if uses != velnor_actions_contract::workflow::step_identity::TOFU_PROVIDER_ADMISSION_USES
            || !env.is_empty()
        {
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

/// Validate an existing push-gated provider save against its restore.
///
/// Mirrors `Save Cargo sources`: the trusted save gate keeps PR runs
/// read-only, and the step archives the exact entry the restore
/// reads, so a push-seeded entry warms every later restore of the
/// key. Jobs that already carry the save keep exactly one.
fn validate_provider_save(job: &Job, key: &str, path: &str) -> Result<(), RenderError> {
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
            || save.condition.as_deref()
                != Some(velnor_actions_contract::workflow::ir::CACHE_SAVE_CONDITION)
        {
            return Err(RenderError::InvalidWorkflow(
                "tofu_provider_save_restore_mismatch".to_owned(),
            ));
        }
        return Ok(());
    }
    Ok(())
}

/// True when one job already carries a typed provider save.
fn has_provider_save(job: &Job) -> bool {
    job.steps
        .iter()
        .any(|step| step.role == Some(StepRole::TofuProvidersSave))
}

/// This job's V2 tools `(key, static digest)` after identity validation.
fn tools_restore_identity(id: &str, job: &Job) -> Result<Option<(String, String)>, RenderError> {
    let restores = job
        .steps
        .iter()
        .filter(|step| step.role == Some(StepRole::ToolsCacheRestore))
        .collect::<Vec<_>>();
    let identities = job
        .steps
        .iter()
        .filter(|step| step.role == Some(StepRole::ToolsCacheIdentity))
        .collect::<Vec<_>>();
    if restores.is_empty() && identities.is_empty() {
        return Ok(None);
    }
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
    if identity.id != Some(velnor_actions_contract::StepId::ToolsCacheIdentity)
        || identity.condition.is_some()
    {
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
