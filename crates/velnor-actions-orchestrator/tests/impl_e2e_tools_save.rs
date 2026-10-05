//! End-to-end V2 tools-cache wiring checks over parsed workflow text.
//!
//! Checks runtime-gated restore shape plus one push-gated writer per key.

use crate::impl_e2e_wiring::{JobText, StepText};

/// Tools-cache step display names, asserted as emitted text.
const RESTORE_TOOLS_TEXT: &str = "Restore Mise tools";
const SAVE_TOOLS_TEXT: &str = "Save Mise tools";
const IDENTITY_TEXT: &str = "V2 identity";
const TOOLS_PATHS: [&str; 5] = [
    "~/.local/share/mise",
    "${{ runner.temp }}/velnor/rustup",
    "${{ runner.temp }}/velnor/cargo/.crates.toml",
    "${{ runner.temp }}/velnor/cargo/.crates2.json",
    "${{ runner.temp }}/velnor/cargo/bin",
];

/// At most one V2 restore and save per job, with exact paths and runtime gates.
pub(crate) fn check_tools_save_shape(job: &JobText) -> Result<(), String> {
    let identities = job
        .steps
        .iter()
        .filter(|step| step.name == IDENTITY_TEXT)
        .count();
    let restores = job
        .steps
        .iter()
        .filter(|step| step.name == RESTORE_TOOLS_TEXT)
        .collect::<Vec<_>>();
    let saves = job
        .steps
        .iter()
        .filter(|step| step.name == SAVE_TOOLS_TEXT)
        .collect::<Vec<_>>();
    if restores.len() > 1 || saves.len() > 1 {
        return Err(format!("{}: at most one V2 restore and save", job.id));
    }
    if restores.is_empty() {
        if identities > 0 || !saves.is_empty() {
            return Err(format!("{}: incomplete V2 tools cache", job.id));
        }
        return Ok(());
    }
    if identities != 1 {
        return Err(format!("{}: one runtime identity per V2 restore", job.id));
    }
    let restore = restores[0];
    check_cache_action(restore, "actions/cache/restore@", job)?;
    if !restore.body.contains("outputs.enabled == 'true'") {
        return Err(format!("{}: restore misses runtime gate", job.id));
    }
    let key = step_key(restore).ok_or_else(|| format!("{}: restore without key", job.id))?;
    if !key.starts_with("mise-tools-v2-") {
        return Err(format!("{}: restore is not V2: {key}", job.id));
    }
    check_paths(restore, job)?;
    for save in saves {
        check_cache_action(save, "actions/cache/save@", job)?;
        if !save
            .body
            .contains("success() && github.event_name == 'push'")
            || !save
                .body
                .contains("github.event_name != 'workflow_dispatch'")
        {
            return Err(format!(
                "{}: tools save lacks push policy or dispatch denial",
                job.id
            ));
        }
        if step_key(save).as_deref() != Some(&key) {
            return Err(format!(
                "{}: save does not archive restored key {key}",
                job.id
            ));
        }
        check_paths(save, job)?;
    }
    Ok(())
}

/// Exactly one `Save Mise tools` step per restored V2 key across the tree.
pub(crate) fn check_one_tools_saver_per_key(jobs: &[JobText]) -> Result<(), String> {
    use std::collections::BTreeMap;
    let restored = restored_tools_keys(jobs);
    let mut saved: BTreeMap<String, usize> = BTreeMap::new();
    for job in jobs {
        for step in job.steps.iter().filter(|step| step.name == SAVE_TOOLS_TEXT) {
            let key =
                step_key(step).ok_or_else(|| format!("{}: tools save without key", job.id))?;
            if !restored.contains(&key) {
                return Err(format!("{}: tools save archives unrestored {key}", job.id));
            }
            *saved.entry(key).or_default() += 1;
        }
    }
    for key in &restored {
        match saved.get(key) {
            Some(1) => {}
            other => return Err(format!("key {key} has {other:?} savers, want exactly one")),
        }
    }
    Ok(())
}

fn check_cache_action(step: &StepText, prefix: &str, job: &JobText) -> Result<(), String> {
    if !step.body.contains(&format!("uses: {prefix}")) {
        return Err(format!("{}: {} must use {prefix}", job.id, step.name));
    }
    Ok(())
}

fn check_paths(step: &StepText, job: &JobText) -> Result<(), String> {
    for path in TOOLS_PATHS {
        if !step.body.contains(path) {
            return Err(format!("{}: {} misses path {path}", job.id, step.name));
        }
    }
    Ok(())
}

/// Every tools key restored by one V2 restore action in the tree.
fn restored_tools_keys(jobs: &[JobText]) -> std::collections::BTreeSet<String> {
    let mut restored = std::collections::BTreeSet::new();
    for job in jobs {
        for step in job
            .steps
            .iter()
            .filter(|step| step.name == RESTORE_TOOLS_TEXT)
        {
            if let Some(key) = step_key(step) {
                restored.insert(key);
            }
        }
    }
    restored
}

/// The cache key expression carried by one cache action.
fn step_key(step: &StepText) -> Option<String> {
    step.body.lines().find_map(|line| {
        line.trim()
            .strip_prefix("key: ")
            .map(|key| key.trim_matches('"').to_owned())
    })
}
