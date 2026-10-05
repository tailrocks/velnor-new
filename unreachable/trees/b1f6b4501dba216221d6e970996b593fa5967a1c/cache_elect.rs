//! Per-key Mise-cache writer election (P08 race closure).
//!
//! Exactly one saver per built-in cache key, elected after every setup
//! step is inserted. Split from `cache_p08` (size gate).

use std::collections::BTreeMap;

use velnor_actions_contract::{Job, StepKind};

use crate::{RenderError, setup::MISE_ACTION_NAME};

/// Elect one Mise-cache writer per cache key across jobs.
///
/// Every qualified setup restores read-only (`cache_save: "false"`: the
/// pinned action saves only inside its `install` leg, which Velnor
/// disables), so without an explicit saver no run would ever warm the
/// shared entries. Exactly one job per key gets a push-gated `Save Mise
/// tools` step over that key; every other sharer restores it read-only.
/// The plan job wins a shared key (it runs first and owns the
/// trusted-writer role); a sole owner's key keeps its writer; any other
/// shared key goes to the lowest job ID, so the election stays
/// deterministic across renders. Setups are never rewritten; reruns add
/// no second save to a job that already carries one.
///
/// # Errors
///
/// Returns [`RenderError`] when a winner's save step fails to build
/// (unreachable for keys read back from valid setups).
pub fn elect_mise_cache_writers(jobs: &mut BTreeMap<String, Job>) -> Result<(), RenderError> {
    let mut by_key: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for (id, job) in jobs.iter() {
        if let Some(key) = setup_cache_key(job) {
            by_key.entry(key).or_default().push(id.clone());
        }
    }
    for (key, owners) in &by_key {
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
        if let Some((key, path)) = provider_restore_entry(job) {
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
        if let Some(job) = jobs.get_mut(winner) {
            append_provider_save(job, key, path)?;
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
        let StepKind::Action { uses, with, .. } = &step.kind else {
            return None;
        };
        if !uses.starts_with("actions/cache/restore@") {
            return None;
        }
        let key = with.get("key")?;
        if key.trim().is_empty() {
            return None;
        }
        let path = with.get("path")?;
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
    if job
        .steps
        .iter()
        .any(|step| step.name == crate::tofu_cache::TOFU_PROVIDERS_SAVE_NAME)
    {
        return Ok(());
    }
    let mut save = crate::tofu_cache::tofu_providers_save_step(key, path)?;
    save.condition = Some(velnor_actions_contract::workflow::ir::CACHE_SAVE_CONDITION.to_owned());
    job.steps.push(save);
    Ok(())
}

/// This job's Mise built-in cache key, when its setup carries one.
fn setup_cache_key(job: &Job) -> Option<String> {
    job.steps.iter().find_map(|step| {
        let StepKind::Action { uses, with, .. } = &step.kind else {
            return None;
        };
        if !uses.starts_with(&format!("{MISE_ACTION_NAME}@")) {
            return None;
        }
        with.get("cache_key").cloned()
    })
}

/// Append the push-gated tools save over `key` to one writer job.
///
/// Mirrors `Save Cargo sources`: the trusted save gate keeps PR runs
/// read-only, and the step archives the default mise data dir the
/// built-in restore reads, so a push-seeded entry warms every later
/// restore of the key. Closures and fan-in steps added after the
/// election install no tools, so the capture stays complete.
fn append_tools_save(job: &mut Job, key: &str) -> Result<(), RenderError> {
    if job
        .steps
        .iter()
        .any(|step| step.name == crate::cache_steps::TOOLS_SAVE_NAME)
    {
        return Ok(());
    }
    let mut save = crate::cache_steps::tools_save_step(key)?;
    save.condition = Some(velnor_actions_contract::workflow::ir::CACHE_SAVE_CONDITION.to_owned());
    job.steps.push(save);
    Ok(())
}
