//! End-to-end explicit tool-cache wiring over parsed workflow text.

use velnor_actions_contract::workflow::ir::CACHE_SAVE_CONDITION;
use velnor_actions_workflow_renderer::steps::{
    TOOLS_CACHE_PATHS, TOOLS_CACHE_RESTORE_CONDITION, TOOLS_CACHE_SAVE_CONDITION,
    TOOLS_RESTORE_USES, TOOLS_SAVE_USES,
};

use crate::impl_e2e_wiring::{JobText, StepText};

const RESTORE_NAME: &str = "Restore Mise tools";
const SAVE_NAME: &str = "Save Mise tools";
const KEY_PREFIX: &str = "mise-v3-";

/// Every tool restore/save is pinned and consumes the canonical payload.
pub(crate) fn check_tools_save_shape(job: &JobText) -> Result<(), String> {
    let restores = job
        .steps
        .iter()
        .filter(|step| step.name == RESTORE_NAME)
        .collect::<Vec<_>>();
    let saves = job
        .steps
        .iter()
        .filter(|step| step.name == SAVE_NAME)
        .collect::<Vec<_>>();
    let has_setup = job.steps.iter().any(|step| step.name == "Setup Mise");
    if restores.len() != usize::from(has_setup) {
        return Err(format!("{}: expected one restore iff setup exists", job.id));
    }
    if saves.len() > 1 {
        return Err(format!("{}: at most one tools save", job.id));
    }
    for restore in restores {
        if !restore
            .body
            .contains(&format!("uses: {TOOLS_RESTORE_USES}"))
            || !restore
                .body
                .contains(&format!("if: {TOOLS_CACHE_RESTORE_CONDITION}"))
            || restore.body.contains("restore-keys:")
        {
            return Err(format!("{}: malformed exact tools restore", job.id));
        }
        check_payload(job, restore)?;
        let key = step_key(job, restore)?;
        if !key.starts_with(KEY_PREFIX) {
            return Err(format!("{}: noncanonical tools key {key}", job.id));
        }
        if let Some(save) = saves.first() {
            if step_key(job, save)? != key {
                return Err(format!("{}: tools save key differs from restore", job.id));
            }
        }
    }
    for save in saves {
        let trusted = format!("{CACHE_SAVE_CONDITION} && {TOOLS_CACHE_SAVE_CONDITION}");
        if !save.body.contains(&format!("uses: {TOOLS_SAVE_USES}"))
            || !save.body.contains(&trusted)
            || !step_key(job, save)?.starts_with(KEY_PREFIX)
        {
            return Err(format!("{}: malformed trusted tools save", job.id));
        }
        check_payload(job, save)?;
    }
    Ok(())
}

/// One explicit elected saver per canonical restored key across the tree.
pub(crate) fn check_one_tools_saver_per_key(jobs: &[JobText]) -> Result<(), String> {
    use std::collections::{BTreeMap, BTreeSet};
    let restored = restored_tools_keys(jobs);
    let mut saved = BTreeMap::<String, usize>::new();
    for job in jobs {
        for step in job.steps.iter().filter(|step| step.name == SAVE_NAME) {
            let key = step_key(job, step)?;
            if !restored.contains(&key) {
                return Err(format!("{}: tools save archives unrestored {key}", job.id));
            }
            *saved.entry(key).or_default() += 1;
        }
    }
    for key in &restored {
        if saved.get(key) != Some(&1) {
            return Err(format!(
                "key {key} has {:?} savers, want one",
                saved.get(key)
            ));
        }
    }
    let saved_keys = saved.keys().collect::<BTreeSet<_>>();
    if saved_keys.len() != restored.len() {
        return Err("tools restore/save key sets differ".to_owned());
    }
    Ok(())
}

/// Every canonical root occurs once and in declaration order.
fn check_payload(job: &JobText, step: &StepText) -> Result<(), String> {
    let mut cursor = 0;
    for path in TOOLS_CACHE_PATHS {
        let Some(at) = step.body[cursor..].find(path) else {
            return Err(format!("{} {:?}: payload misses {path}", job.id, step.name));
        };
        cursor += at + path.len();
    }
    Ok(())
}

/// Read the canonical action-key input from one rendered step.
fn step_key(job: &JobText, step: &StepText) -> Result<String, String> {
    step.body
        .lines()
        .find_map(|line| line.trim().strip_prefix("key: "))
        .map(str::to_owned)
        .ok_or_else(|| format!("{} {:?}: missing key", job.id, step.name))
}

/// The set of keys explicitly restored by tool-cache steps.
fn restored_tools_keys(jobs: &[JobText]) -> std::collections::BTreeSet<String> {
    jobs.iter()
        .flat_map(|job| {
            job.steps
                .iter()
                .filter(|step| step.name == RESTORE_NAME)
                .filter_map(|step| step_key(job, step).ok())
        })
        .collect()
}
