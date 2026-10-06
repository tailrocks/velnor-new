//! End-to-end tools-save wiring checks over parsed workflow text.
//!
//! Split from `impl_e2e_wiring` (size gate): per-job save shape plus the
//! tree-wide one-saver-per-key check.

use crate::impl_e2e_wiring::{JobText, StepText};

/// Tools-cache step display names, asserted as emitted text.
const RESTORE_TOOLS_TEXT: &str = "Restore Mise tools";
const SAVE_TOOLS_TEXT: &str = "Save Mise tools";

/// Tools restores stay built-in (never a manual restore step, never a
/// role-suffixed `mise-tools-v1-` key); at most one `Save Mise tools`
/// step per job, carrying the shared `mise-v1-` key under the push-only
/// gate (P08: one saver per key, PRs read-only).
pub(crate) fn check_tools_save_shape(job: &JobText) -> Result<(), String> {
    let mut saves = 0;
    for step in &job.steps {
        if step.name == RESTORE_TOOLS_TEXT {
            return Err(format!("{}: tools restores stay built-in", job.id));
        }
        if step.body.contains("key: mise-tools-v1-") {
            return Err(format!("{}: role-suffixed tools key must go", job.id));
        }
        if step.name == SAVE_TOOLS_TEXT {
            saves += 1;
            for need in [
                "key: mise-v1-",
                "if: success() && github.event_name == 'push'",
            ] {
                if !step.body.contains(need) {
                    return Err(format!("{}: tools save misses {need}", job.id));
                }
            }
        }
    }
    if saves > 1 {
        return Err(format!("{}: at most one tools save", job.id));
    }
    Ok(())
}

/// Exactly one `Save Mise tools` step per restored `mise-v1-` key across
/// the tree, archiving that same key.
pub(crate) fn check_one_tools_saver_per_key(jobs: &[JobText]) -> Result<(), String> {
    use std::collections::BTreeMap;
    let restored = restored_tools_keys(jobs);
    let mut saved: BTreeMap<String, usize> = BTreeMap::new();
    for job in jobs {
        for step in job.steps.iter().filter(|s| s.name == SAVE_TOOLS_TEXT) {
            let key = save_step_key(job, step)?;
            if !restored.contains(&key) {
                return Err(format!("{}: tools save archives unrestored {key}", job.id));
            }
            *saved.entry(key).or_default() += 1;
        }
    }
    for key in &restored {
        match saved.get(key) {
            Some(1) => {}
            other => {
                return Err(format!("key {key} has {other:?} savers, want exactly one"));
            }
        }
    }
    Ok(())
}

/// Every `mise-v1-` key restored by a `Setup Mise` step in the tree.
fn restored_tools_keys(jobs: &[JobText]) -> std::collections::BTreeSet<String> {
    let mut restored = std::collections::BTreeSet::new();
    for job in jobs {
        for step in job.steps.iter().filter(|s| s.name == "Setup Mise") {
            for line in step.body.lines() {
                if let Some(key) = line.trim().strip_prefix("cache_key: ") {
                    restored.insert(key.to_owned());
                }
            }
        }
    }
    restored
}

/// The archived key of one tools save step body.
fn save_step_key(job: &JobText, step: &StepText) -> Result<String, String> {
    for line in step.body.lines() {
        if let Some(key) = line.trim().strip_prefix("key: ") {
            return Ok(key.to_owned());
        }
    }
    Err(format!("{}: tools save without key", job.id))
}
