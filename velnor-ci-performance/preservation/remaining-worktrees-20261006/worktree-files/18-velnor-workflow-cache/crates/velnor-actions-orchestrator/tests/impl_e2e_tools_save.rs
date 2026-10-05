//! End-to-end tools-save wiring checks over parsed workflow text.
//!
//! Split from `impl_e2e_wiring` (size gate): per-job save shape plus the
//! tree-wide one-saver-per-key check.

use crate::impl_e2e_wiring::{JobText, StepText};
use velnor_actions_mise::{PinnedTool, ToolCatalog};

/// Tools-cache step display names, asserted as emitted text.
const RESTORE_TOOLS_TEXT: &str = "Restore Mise tools";
const SAVE_TOOLS_TEXT: &str = "Save Mise tools";
const SETUP_MISE_TEXT: &str = "Setup Mise";
const PREPARE_TOOLS_TEXT: &str = "Prepare pinned tools";
const TOOLS_PATHS: [&str; 3] = [
    ".velnor/cache/mise",
    ".velnor/cache/rustup",
    ".velnor/cache/cargo/bin",
];

/// One explicit tools restore precedes each cache-disabled setup; at most
/// one matching save per job runs under the push-only gate.
pub(crate) fn check_tools_save_shape(job: &JobText) -> Result<(), String> {
    let setups: Vec<&StepText> = job
        .steps
        .iter()
        .filter(|step| step.name == SETUP_MISE_TEXT)
        .collect();
    let restores: Vec<&StepText> = job
        .steps
        .iter()
        .filter(|step| step.name == RESTORE_TOOLS_TEXT)
        .collect();
    if setups.len() != restores.len() {
        return Err(format!(
            "{}: Setup Mise and tools restore counts differ ({} setups, {} restores)",
            job.id,
            setups.len(),
            restores.len()
        ));
    }
    for (restore, setup) in restores.iter().zip(&setups) {
        let restore_key = step_key(restore, "key")
            .ok_or_else(|| format!("{}: explicit tools restore without key", job.id))?;
        let setup_key = step_key(setup, "cache_key")
            .ok_or_else(|| format!("{}: Mise setup without cache key", job.id))?;
        let restore_at = job
            .steps
            .iter()
            .position(|step| std::ptr::eq(step, *restore))
            .ok_or_else(|| format!("{}: missing tools restore", job.id))?;
        let setup_at = job
            .steps
            .iter()
            .position(|step| std::ptr::eq(step, *setup))
            .ok_or_else(|| format!("{}: missing Mise setup", job.id))?;
        if restore_at + 1 != setup_at {
            return Err(format!(
                "{}: explicit tools restore must immediately precede Setup Mise",
                job.id
            ));
        }
        if let Some(prepare_at) = job
            .steps
            .iter()
            .position(|step| step.name == PREPARE_TOOLS_TEXT)
        {
            if !(restore_at < setup_at && setup_at < prepare_at) {
                return Err(format!(
                    "{}: tools restore and setup must precede pinned-tool preparation",
                    job.id
                ));
            }
            let prepare = &job.steps[prepare_at];
            if !setup
                .body
                .contains("MISE_DATA_DIR: ${{ github.workspace }}/.velnor/cache/mise")
                || !prepare
                    .body
                    .contains("MISE_DATA_DIR: ${{ github.workspace }}/.velnor/cache/mise")
            {
                return Err(format!(
                    "{}: Setup Mise and preparation need the explicit workspace Mise data dir",
                    job.id
                ));
            }
            let rust = ToolCatalog::pinned().tool_spec(PinnedTool::Rust);
            if prepare.body.contains("MISE_RUSTUP_HOME:") && !prepare.body.contains(rust.as_str()) {
                return Err(format!(
                    "{}: Rust preparation must install exact pin {rust}",
                    job.id
                ));
            }
        }
        if restore_key != setup_key {
            return Err(format!(
                "{}: explicit restore key {restore_key} differs from setup {setup_key}",
                job.id
            ));
        }
        if !restore_key.starts_with("mise-v1-") {
            return Err(format!("{}: tools restore key is not qualified", job.id));
        }
        if !restore.body.contains("actions/cache/restore@") {
            return Err(format!("{}: tools restore is not explicit", job.id));
        }
        if restore.body.contains("restore-keys:") {
            return Err(format!("{}: tools restore must use an exact key", job.id));
        }
        if !has_exact_tool_paths(restore) {
            return Err(format!("{}: tools restore path set is incomplete", job.id));
        }
        if !setup.body.contains("cache: \"false\"") {
            return Err(format!(
                "{}: Setup Mise built-in cache must be disabled",
                job.id
            ));
        }
        if !setup.body.contains("cache_save: \"false\"") || setup.body.contains("cache: \"true\"") {
            return Err(format!(
                "{}: Setup Mise built-in cache must stay off",
                job.id
            ));
        }
    }

    let mut saves = 0;
    for step in &job.steps {
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
            if !has_exact_tool_paths(step) {
                return Err(format!("{}: tools save path set is incomplete", job.id));
            }
            let key = step_key(step, "key")
                .ok_or_else(|| format!("{}: tools save without key", job.id))?;
            let matching_restore = restores
                .iter()
                .find(|restore| step_key(restore, "key").as_deref() == Some(key.as_str()))
                .ok_or_else(|| format!("{}: tools save without matching restore", job.id))?;
            if cache_paths(matching_restore) != cache_paths(step) {
                return Err(format!("{}: tools save path differs from restore", job.id));
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
            let restore = jobs
                .iter()
                .flat_map(|job| &job.steps)
                .find(|candidate| {
                    candidate.name == RESTORE_TOOLS_TEXT
                        && step_key(candidate, "key").as_deref() == Some(key.as_str())
                })
                .ok_or_else(|| format!("{key}: missing explicit tools restore"))?;
            if cache_paths(restore) != cache_paths(step) {
                return Err(format!("{}: tools save path differs from restore", job.id));
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

/// Every `mise-v1-` key carried by an explicit tools restore in the tree.
fn restored_tools_keys(jobs: &[JobText]) -> std::collections::BTreeSet<String> {
    let mut restored = std::collections::BTreeSet::new();
    for job in jobs {
        for step in job
            .steps
            .iter()
            .filter(|step| step.name == RESTORE_TOOLS_TEXT)
        {
            if let Some(key) = step_key(step, "key") {
                restored.insert(key);
            }
        }
    }
    restored
}

/// Read an action input key from one rendered step.
fn step_key(step: &StepText, field: &str) -> Option<String> {
    let prefix = format!("{field}: ");
    step.body
        .lines()
        .find_map(|line| line.trim().strip_prefix(&prefix).map(str::to_owned))
}

/// Exact paths carried by one rendered tools-cache step.
fn cache_paths(step: &StepText) -> Vec<&'static str> {
    TOOLS_PATHS
        .into_iter()
        .filter(|path| step.body.lines().any(|line| line.trim() == *path))
        .collect()
}

/// True when one step carries exactly the complete tools-cache path set.
fn has_exact_tool_paths(step: &StepText) -> bool {
    cache_paths(step) == TOOLS_PATHS
}

/// The archived key of one tools save step body.
fn save_step_key(job: &JobText, step: &StepText) -> Result<String, String> {
    step_key(step, "key").ok_or_else(|| format!("{}: tools save without key", job.id))
}
