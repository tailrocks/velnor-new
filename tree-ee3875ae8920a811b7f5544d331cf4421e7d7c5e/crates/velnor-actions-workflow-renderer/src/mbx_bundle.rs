//! Manual MBX bundle save when the pinned action does not own that lane.
//!
//! `ACTIONS_CACHE_MODE=read` keeps the pinned action from exporting inside the
//! live store. The workflow exports one bundle under `runner.temp` and saves
//! only that path. A fresh per-job store root prevents one Scale Set job from
//! writing into another job's MBX state. The store remains until normal runner
//! temporary-directory cleanup; export success is not a store-wide lease.
//! A miss, a missing directory, or a failed import continues the job cold.
//! Import and export print byte and inode lines for `$RUNNER_TEMP`.

use std::collections::BTreeMap;

use velnor_actions_contract::{Job, Step, StepKind};

use crate::RenderError;
use crate::cache_steps::{
    MBX_ACTION_CACHE_MODE, MBX_ACTION_NAME, MBX_CACHE_MODE_ENV, MBX_PREFLIGHT_NAME,
    MBX_RESTORE_NAME, TOOLS_RESTORE_USES, TOOLS_SAVE_USES, is_mbx_action,
};
use crate::matrix::MATRIX_NEEDS_JOB_ENV;

#[path = "mbx_bundle_store.rs"]
mod store;
use store::{EXPORT_SCRIPT, HOSTED_STORE_ISOLATION, SCALE_SET_ONLY_IF, STORE_INIT_SCRIPT};

/// Display name of the reclaim-and-export step.
pub(crate) const MBX_BUNDLE_EXPORT_NAME: &str = "Export MBX single bundle";
/// Display name of the one-file cache save.
pub(crate) const MBX_BUNDLE_SAVE_NAME: &str = "Save MBX single bundle";
/// Display name of the prefix step ahead of the bundle restore.
pub(crate) const MBX_BUNDLE_KEY_NAME: &str = "Prepare MBX bundle key";
/// Display name of the bundle restore. Same path the save archived.
pub(crate) const MBX_BUNDLE_RESTORE_NAME: &str = "Restore MBX single bundle";
/// Display name of the import into the mbx store.
pub(crate) const MBX_BUNDLE_IMPORT_NAME: &str = "Import MBX single bundle";
/// Display name of the fresh per-job MBX store setup.
pub(crate) const MBX_STORE_INIT_NAME: &str = "Initialize private MBX store";
/// Restored archive path, retained through runner-temp cleanup after import.
pub(crate) const MBX_BUNDLE_RESTORE_PATH: &str = "${{ runner.temp }}/mbx-single-bundle-restore";
/// Export path outside the mbx store. `actions/cache` archives only this path.
pub(crate) const MBX_BUNDLE_PATH: &str = "${{ runner.temp }}/mbx-single-bundle-export";
/// Push-only export gate. Exact bundle hits skip; pull requests never reach it.
const PREP_IF: &str = "success() && github.event_name == 'push' && steps.mbx.outputs.cache-hit != 'true' && steps.mbx-bundle.outputs.cache-hit != 'true'";
/// Save gate. Empty exports set `ready=false` and must not call `actions/cache`.
const SAVE_IF: &str = "success() && github.event_name == 'push' && steps.mbx.outputs.cache-hit != 'true' && steps.mbx-bundle.outputs.cache-hit != 'true' && steps.mbx-export.outputs.ready == 'true'";
/// Separate attempts, then restore by stable writer identity before common compatibility.
const KEY_SCRIPT: &str = r#"set -eu; key="$MBX_KEY"; job_id="$MBX_JOB_ID"; matrix_key="$MBX_MATRIX_KEY"; run_id="$MBX_RUN_ID"; run_attempt="$MBX_RUN_ATTEMPT"; case "$key" in ''|*-) exit 1 ;; esac; case "$job_id" in ''|*[!A-Za-z0-9_-]*) exit 1 ;; esac; case "$run_id" in ''|*[!0-9]*) exit 1 ;; esac; case "$run_attempt" in ''|*[!0-9]*) exit 1 ;; esac; job_identity="j${#job_id}-${job_id}"; if [ -n "$matrix_key" ]; then case "$matrix_key" in m-????????????????) ;; *) exit 1 ;; esac; case "${matrix_key#m-}" in *[!a-f0-9]*) exit 1 ;; esac; writer_identity="${job_identity}-m-${matrix_key}"; else writer_identity="${job_identity}-n"; fi; compatibility_key="${key%-*}"; writer_prefix="${compatibility_key}-${writer_identity}-"; primary="${writer_prefix}r${run_id}-a${run_attempt}"; [ "${#primary}" -le 512 ]; prefix="$writer_prefix"; fallback="${compatibility_key}-"; printf 'key=%s\nprefix=%s\nfallback=%s\n' "$primary" "$prefix" "$fallback" >> "$GITHUB_OUTPUT""#;
/// Import when the restore matched. A miss, a missing directory, or a failed import stays cold.
const IMPORT_SCRIPT: &str = r#"set -eu
bundle="$RUNNER_TEMP/mbx-single-bundle-restore"
cache_unavailable() {
    reason="$1"
    report_cache_failure "cache_unavailable" "$reason"
}
cache_corrupt() {
    reason="$1"
    report_cache_failure "cache_corrupt" "$reason"
}
report_cache_failure() {
    acceptance="$1"
    reason="$2"
    if [ -n "${GITHUB_ENV:-}" ]; then
        printf 'MBX_CACHE_IMPORT_UNAVAILABLE=true\n' >> "$GITHUB_ENV" || true
    fi
    if [ -n "${GITHUB_OUTPUT:-}" ]; then
        printf 'ready=false\nacceptance=%s\n' "$acceptance" >> "$GITHUB_OUTPUT" || true
    fi
    printf '::warning::MBX cache acceptance failed: %s\n' "$reason" >&2
    printf 'MBX cache acceptance: %s (%s)\n' "$acceptance" "$reason" \
        >> "${GITHUB_STEP_SUMMARY:-/dev/null}" || true
    exit 0
}

if ! df -B1 -P "$RUNNER_TEMP" || ! df -i -P "$RUNNER_TEMP"; then
    cache_unavailable "disk-measurement-failed"
fi
if [ -z "${MATCHED:-}" ]; then
    echo "no mbx bundle matched"
    exit 0
fi
if [ -L "$bundle" ]; then
    cache_unavailable "matched-bundle-missing-or-symlink"
fi
if [ ! -d "$bundle" ]; then
    cache_corrupt "matched-bundle-missing"
fi
if ! mbx cache import "$bundle"; then
    echo "mbx bundle import failed; continuing cold"
    cache_corrupt "bundle-import-failed"
fi"#;

/// YAML step id for the MBX restore and export steps, when they have one.
pub(crate) fn step_yaml_id(name: &str) -> Option<&'static str> {
    match name {
        MBX_RESTORE_NAME => Some("mbx"),
        MBX_BUNDLE_KEY_NAME => Some("mbx-bundle-key"),
        MBX_BUNDLE_RESTORE_NAME => Some("mbx-bundle"),
        MBX_BUNDLE_EXPORT_NAME => Some("mbx-export"),
        _ => None,
    }
}

/// Emit `id:` for the two MBX steps whose later steps read outputs.
pub(crate) fn push_step_id(entries: &mut Vec<(String, crate::yaml::Yaml)>, name: &str) {
    let Some(id) = step_yaml_id(name) else {
        return;
    };
    entries.push(("id".to_owned(), crate::yaml::Yaml::str(id.to_owned())));
}

/// Force every MBX action to restore-only and append the single-bundle save.
///
/// # Errors
///
/// Returns [`RenderError`] when the export or save step fails validation.
pub(crate) fn append_single_bundle_saves(
    jobs: &mut BTreeMap<String, Job>,
) -> Result<(), RenderError> {
    for (job_id, job) in jobs.iter_mut() {
        if !job.steps.iter().any(is_mbx_action) {
            continue;
        }
        let action_owns_hosted_store = action_owns_hosted_store(&job.steps)?;
        pin_restore_only(&mut job.steps, action_owns_hosted_store);
        insert_private_store_init(job, action_owns_hosted_store)?;
        let matrix_job = is_matrix_job(job_id, job);
        insert_bundle_restore(job, matrix_job)?;
        if !job
            .steps
            .iter()
            .any(|step| step.name == MBX_BUNDLE_EXPORT_NAME)
        {
            job.steps.push(export_step()?);
            job.steps.push(save_step()?);
        }
        if action_owns_hosted_store {
            scope_bundle_route_to_scale_set(job);
        }
    }
    Ok(())
}

fn scope_bundle_route_to_scale_set(job: &mut Job) {
    let export_condition = format!("{SCALE_SET_ONLY_IF} && {PREP_IF}");
    let save_condition = format!("{SCALE_SET_ONLY_IF} && {SAVE_IF}");
    for step in &mut job.steps {
        match step.name.as_str() {
            MBX_BUNDLE_KEY_NAME | MBX_BUNDLE_RESTORE_NAME | MBX_BUNDLE_IMPORT_NAME => {
                step.condition = Some(SCALE_SET_ONLY_IF.to_owned());
            }
            MBX_BUNDLE_EXPORT_NAME => step.condition = Some(export_condition.clone()),
            MBX_BUNDLE_SAVE_NAME => step.condition = Some(save_condition.clone()),
            _ => {}
        }
    }
}

fn action_owns_hosted_store(steps: &[Step]) -> Result<bool, RenderError> {
    for step in steps {
        let StepKind::Action { uses, with, .. } = &step.kind else {
            continue;
        };
        if !uses.starts_with(&format!("{MBX_ACTION_NAME}@")) {
            continue;
        }
        let Some(isolation) = with.get("isolate-objects-cache") else {
            continue;
        };
        if isolation != HOSTED_STORE_ISOLATION {
            return Err(RenderError::InvalidWorkflow(
                "unsupported_mbx_store_isolation_input".to_owned(),
            ));
        }
        return Ok(true);
    }
    Ok(false)
}

fn insert_private_store_init(
    job: &mut Job,
    action_owns_hosted_store: bool,
) -> Result<(), RenderError> {
    if job
        .steps
        .iter()
        .any(|step| step.name == MBX_STORE_INIT_NAME)
    {
        return Ok(());
    }
    let at = job
        .steps
        .iter()
        .position(|step| step.name == MBX_PREFLIGHT_NAME)
        .or_else(|| job.steps.iter().position(is_mbx_action))
        .ok_or_else(|| RenderError::InvalidWorkflow("mbx_store_init_without_action".to_owned()))?;
    let step = private_store_init_step(action_owns_hosted_store)?;
    job.steps.insert(at, step);
    Ok(())
}

fn private_store_init_step(action_owns_hosted_store: bool) -> Result<Step, RenderError> {
    let mut step = crate::steps::shell_step(
        MBX_STORE_INIT_NAME,
        vec![
            "bash".to_owned(),
            "-c".to_owned(),
            STORE_INIT_SCRIPT.to_owned(),
        ],
        BTreeMap::new(),
    )?;
    if action_owns_hosted_store {
        step.condition = Some(SCALE_SET_ONLY_IF.to_owned());
    }
    Ok(step)
}

fn pin_restore_only(steps: &mut [Step], action_owns_hosted_store: bool) {
    if action_owns_hosted_store {
        return;
    }
    for step in steps {
        let StepKind::Action { uses, env, .. } = &mut step.kind else {
            continue;
        };
        if !uses.starts_with(&format!("{MBX_ACTION_NAME}@")) {
            continue;
        }
        env.insert(
            MBX_CACHE_MODE_ENV.to_owned(),
            MBX_ACTION_CACHE_MODE.to_owned(),
        );
    }
}

fn is_matrix_job(job_id: &str, job: &Job) -> bool {
    job_id == crate::render::TASK_JOB_ID
        && job.steps.iter().any(|step| match &step.kind {
            StepKind::Shell { env, .. } => env.contains_key(MATRIX_NEEDS_JOB_ENV),
            _ => false,
        })
}

fn insert_bundle_restore(job: &mut Job, matrix_job: bool) -> Result<(), RenderError> {
    if job
        .steps
        .iter()
        .any(|step| step.name == MBX_BUNDLE_RESTORE_NAME)
    {
        return Ok(());
    }
    let Some(index) = job.steps.iter().position(is_mbx_action) else {
        return Ok(());
    };
    let at = index + 1;
    let added = [key_step(matrix_job)?, restore_step()?, import_step()?];
    job.steps.splice(at..at, added);
    Ok(())
}

fn key_step(matrix_job: bool) -> Result<Step, RenderError> {
    let mut env = BTreeMap::from([
        (
            "MBX_KEY".to_owned(),
            "${{ steps.mbx.outputs.cache-primary-key }}".to_owned(),
        ),
        ("MBX_JOB_ID".to_owned(), "${{ github.job }}".to_owned()),
        ("MBX_RUN_ID".to_owned(), "${{ github.run_id }}".to_owned()),
        (
            "MBX_RUN_ATTEMPT".to_owned(),
            "${{ github.run_attempt }}".to_owned(),
        ),
    ]);
    if matrix_job {
        env.insert(
            "MBX_MATRIX_KEY".to_owned(),
            "${{ matrix.matrix_key }}".to_owned(),
        );
    } else {
        env.insert("MBX_MATRIX_KEY".to_owned(), String::new());
    }
    crate::steps::shell_step(
        MBX_BUNDLE_KEY_NAME,
        vec!["bash".to_owned(), "-c".to_owned(), KEY_SCRIPT.to_owned()],
        env,
    )
}

fn restore_step() -> Result<Step, RenderError> {
    crate::steps::action_step(
        MBX_BUNDLE_RESTORE_NAME,
        TOOLS_RESTORE_USES,
        BTreeMap::from([
            (
                "key".to_owned(),
                "${{ steps.mbx-bundle-key.outputs.key }}".to_owned(),
            ),
            (
                "restore-keys".to_owned(),
                "${{ steps.mbx-bundle-key.outputs.prefix }}\n${{ steps.mbx-bundle-key.outputs.fallback }}"
                    .to_owned(),
            ),
            ("path".to_owned(), MBX_BUNDLE_RESTORE_PATH.to_owned()),
        ]),
    )
}

fn import_step() -> Result<Step, RenderError> {
    let env = BTreeMap::from([(
        "MATCHED".to_owned(),
        "${{ steps.mbx-bundle.outputs.cache-matched-key }}".to_owned(),
    )]);
    crate::steps::shell_step(
        MBX_BUNDLE_IMPORT_NAME,
        vec!["bash".to_owned(), "-c".to_owned(), IMPORT_SCRIPT.to_owned()],
        env,
    )
}

fn export_step() -> Result<Step, RenderError> {
    let mut step = crate::steps::shell_step(
        MBX_BUNDLE_EXPORT_NAME,
        vec!["bash".to_owned(), "-c".to_owned(), EXPORT_SCRIPT.to_owned()],
        BTreeMap::new(),
    )?;
    step.condition = Some(PREP_IF.to_owned());
    Ok(step)
}

fn save_step() -> Result<Step, RenderError> {
    let mut step = crate::steps::action_step(
        MBX_BUNDLE_SAVE_NAME,
        TOOLS_SAVE_USES,
        BTreeMap::from([
            (
                "key".to_owned(),
                "${{ steps.mbx-bundle-key.outputs.key }}".to_owned(),
            ),
            ("path".to_owned(), MBX_BUNDLE_PATH.to_owned()),
        ]),
    )?;
    step.condition = Some(SAVE_IF.to_owned());
    Ok(step)
}

#[cfg(test)]
#[path = "mbx_bundle_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "mbx_store_tests.rs"]
mod store_tests;

#[cfg(test)]
#[path = "mbx_store_policy_tests.rs"]
mod store_policy_tests;
