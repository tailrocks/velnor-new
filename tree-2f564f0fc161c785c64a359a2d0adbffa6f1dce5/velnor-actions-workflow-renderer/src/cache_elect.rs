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
