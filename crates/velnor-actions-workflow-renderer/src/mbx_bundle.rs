//! Hosted action-owned MBX isolation and Scale Set single-bundle saves.
//!
//! Hosted jobs use the action-owned objects backend and write only on a
//! protected default-branch push. Typed Scale Set jobs use the action's local
//! backend plus an independently keyed external bundle route. A miss, a
//! missing directory, or a failed import continues the job cold. Import and
//! export print byte and inode lines for `$RUNNER_TEMP`.

use std::collections::BTreeMap;

use velnor_actions_contract::{Job, PullRequestCachePolicy, RunsOn, Step, StepKind};

use crate::RenderError;
use crate::cache_steps::{
    MBX_ACTION_NAME, MBX_CACHE_MODE_ENV, MBX_RESTORE_NAME, TOOLS_RESTORE_USES, TOOLS_SAVE_USES,
    is_mbx_action,
};
#[path = "mbx_bundle_pr_cache.rs"]
mod pr_cache;

/// Display name of the reclaim-and-export step.
pub(crate) const MBX_BUNDLE_EXPORT_NAME: &str = "Export MBX single bundle";
/// Display name of the one-file cache save.
pub(crate) const MBX_BUNDLE_SAVE_NAME: &str = "Save MBX single bundle";
/// Display name of the typed MBX cache identity step.
pub(crate) const MBX_CACHE_KEY_NAME: &str = "Prepare MBX cache identity";
/// Local backend setup step emitted for Scale Set jobs.
pub(crate) const MBX_LOCAL_SETUP_NAME: &str = "Prepare MBX local cache store";
/// Display name of the bundle restore. Same path the save archived.
pub(crate) const MBX_BUNDLE_RESTORE_NAME: &str = "Restore MBX single bundle";
/// Display name of the import into the mbx store.
pub(crate) const MBX_BUNDLE_IMPORT_NAME: &str = "Import MBX single bundle";
/// Bundle path outside the mbx store. `actions/cache` archives only this path.
pub(crate) const MBX_BUNDLE_PATH: &str = "${{ runner.temp }}/mbx-single-bundle";
/// One-line script: gc, one external bundle, delete the store only after it exists.
const EXPORT_SCRIPT: &str = r#"set -eu; df -B1 -P "$RUNNER_TEMP"; df -i -P "$RUNNER_TEMP"; mbx gc; mbx cache dir > "$RUNNER_TEMP/mbx-store-path"; IFS= read -r store < "$RUNNER_TEMP/mbx-store-path"; test -n "$store"; bundle="$RUNNER_TEMP/mbx-single-bundle"; case "$bundle" in "$store"|"$store"/*) exit 1 ;; esac; case "$store" in /|.) exit 1 ;; *mbx*) ;; *) exit 1 ;; esac; rm -rf "$bundle"; if mbx cache export --group "$MBX_CACHE_EXPORT_GROUP" --format directory "$bundle" >"$RUNNER_TEMP/mbx-export.out" 2>&1; then test -d "$bundle"; rm -rf "$store"; df -B1 -P "$RUNNER_TEMP"; df -i -P "$RUNNER_TEMP"; echo "ready=true" >> "$GITHUB_OUTPUT"; else rm -rf "$bundle"; if grep -q "no completed mbx builds are recorded for export group" "$RUNNER_TEMP/mbx-export.out"; then echo "ready=false" >> "$GITHUB_OUTPUT"; exit 0; fi; cat "$RUNNER_TEMP/mbx-export.out"; exit 1; fi"#;
/// Import when the restore matched. A miss, a missing directory, or a failed import stays cold.
const IMPORT_SCRIPT: &str = r#"set -eu; bundle="$RUNNER_TEMP/mbx-single-bundle"; df -B1 -P "$RUNNER_TEMP"; df -i -P "$RUNNER_TEMP"; if [ -z "$MATCHED" ]; then echo "no mbx bundle matched"; exit 0; fi; if [ ! -d "$bundle" ]; then echo "mbx bundle missing; continuing cold"; exit 0; fi; if ! mbx cache import "$bundle"; then echo "mbx bundle import failed; continuing cold"; rm -rf "$bundle"; exit 0; fi"#;

/// YAML step id for the MBX restore and export steps, when they have one.
pub(crate) fn step_yaml_id(name: &str) -> Option<&'static str> {
    match name {
        MBX_RESTORE_NAME => Some("mbx"),
        MBX_CACHE_KEY_NAME => Some("mbx-cache-key"),
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

/// Gate hosted cache writes and append the Scale Set single-bundle save.
///
/// # Errors
///
/// Returns [`RenderError`] when the export or save step fails validation.
pub(crate) fn append_single_bundle_saves(
    jobs: &mut BTreeMap<String, Job>,
    pull_request_cache_policy: PullRequestCachePolicy,
) -> Result<(), RenderError> {
    for job in jobs.values_mut() {
        if !job.steps.iter().any(is_mbx_action) {
            continue;
        }
        let (scale_set, job_policy) =
            configure_runner_scoped_cache(job, pull_request_cache_policy)?;
        if !scale_set {
            continue;
        }
        insert_bundle_restore(job)?;
        if job
            .steps
            .iter()
            .any(|step| step.name == MBX_BUNDLE_EXPORT_NAME)
        {
            continue;
        }
        job.steps.push(export_step(job_policy)?);
        job.steps.push(save_step(job_policy)?);
    }
    Ok(())
}

fn configure_runner_scoped_cache(
    job: &mut Job,
    pull_request_cache_policy: PullRequestCachePolicy,
) -> Result<(bool, PullRequestCachePolicy), RenderError> {
    let runner = RunsOn::parse(&job.runs_on).map_err(RenderError::Contract)?;
    let scale_set = matches!(runner, RunsOn::ScaleSet(_));
    let Some(action_index) = job.steps.iter().position(is_mbx_action) else {
        return Ok((scale_set, pull_request_cache_policy));
    };
    let (action_sha, mbx_version, rust_toolchain, job_policy) = {
        let StepKind::Action { uses, with, env } = &job.steps[action_index].kind else {
            return Err(RenderError::InvalidWorkflow(
                "mbx_bundle_bad_action".to_owned(),
            ));
        };
        let action_sha = uses
            .strip_prefix(&format!("{MBX_ACTION_NAME}@"))
            .ok_or_else(|| RenderError::InvalidWorkflow("mbx_bundle_bad_action".to_owned()))?
            .to_owned();
        let mbx_version = env
            .get("VELNOR_MBX_VERSION")
            .cloned()
            .ok_or_else(|| RenderError::InvalidWorkflow("mbx_missing_version".to_owned()))?;
        let rust_toolchain = with
            .get("toolchain")
            .cloned()
            .ok_or_else(|| RenderError::InvalidWorkflow("mbx_missing_toolchain".to_owned()))?;
        (
            action_sha,
            mbx_version,
            rust_toolchain,
            pr_cache::effective_policy(with, pull_request_cache_policy),
        )
    };
    if !job.steps.iter().any(|step| step.name == MBX_CACHE_KEY_NAME) {
        let key = pr_cache::key_step(
            &action_sha,
            &mbx_version,
            &rust_toolchain,
            scale_set,
            job_policy,
        )?;
        job.steps.insert(action_index, key);
    }
    let Some(action_index) = job.steps.iter().position(is_mbx_action) else {
        return Err(RenderError::InvalidWorkflow(
            "mbx_bundle_missing_action".to_owned(),
        ));
    };
    let generation = velnor_actions_contract::cachekey::mbx_cache_generation(&mbx_version);
    let step = &mut job.steps[action_index];
    let StepKind::Action { with, env, .. } = &mut step.kind else {
        return Err(RenderError::InvalidWorkflow(
            "mbx_bundle_bad_action".to_owned(),
        ));
    };
    with.insert(
        "cache-generation".to_owned(),
        format!("{generation}-share-out-dir-disabled-v1-action-{action_sha}"),
    );
    if scale_set {
        // `local` returns before the pinned action's object restore and
        // leaves the object format to MBX's separate bundle route.
        with.insert("backend".to_owned(), "local".to_owned());
        with.remove("cache-key");
        with.remove("restore-keys");
        env.remove(MBX_CACHE_MODE_ENV);
        MBX_LOCAL_SETUP_NAME.clone_into(&mut step.name);
    } else {
        with.insert("backend".to_owned(), "github".to_owned());
        with.insert(
            "cache-key".to_owned(),
            "${{ steps.mbx-cache-key.outputs.key }}".to_owned(),
        );
        with.insert(
            "restore-keys".to_owned(),
            "${{ steps.mbx-cache-key.outputs.prefix }}".to_owned(),
        );
        env.insert(
            MBX_CACHE_MODE_ENV.to_owned(),
            pr_cache::cache_mode_expression(job_policy),
        );
    }
    Ok((scale_set, job_policy))
}

fn insert_bundle_restore(job: &mut Job) -> Result<(), RenderError> {
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
    let added = [restore_step()?, import_step()?];
    job.steps.splice(at..at, added);
    Ok(())
}

fn restore_step() -> Result<Step, RenderError> {
    let step = crate::steps::action_step(
        MBX_BUNDLE_RESTORE_NAME,
        TOOLS_RESTORE_USES,
        BTreeMap::from([
            (
                "key".to_owned(),
                "${{ steps.mbx-cache-key.outputs.key }}".to_owned(),
            ),
            (
                "restore-keys".to_owned(),
                "${{ steps.mbx-cache-key.outputs.prefix }}".to_owned(),
            ),
            ("path".to_owned(), MBX_BUNDLE_PATH.to_owned()),
        ]),
    )?;
    Ok(step)
}

fn import_step() -> Result<Step, RenderError> {
    let env = BTreeMap::from([(
        "MATCHED".to_owned(),
        "${{ steps.mbx-bundle.outputs.cache-matched-key }}".to_owned(),
    )]);
    let step = crate::steps::shell_step(
        MBX_BUNDLE_IMPORT_NAME,
        vec!["bash".to_owned(), "-c".to_owned(), IMPORT_SCRIPT.to_owned()],
        env,
    )?;
    Ok(step)
}

fn export_step(policy: PullRequestCachePolicy) -> Result<Step, RenderError> {
    let mut step = crate::steps::shell_step(
        MBX_BUNDLE_EXPORT_NAME,
        vec!["bash".to_owned(), "-c".to_owned(), EXPORT_SCRIPT.to_owned()],
        BTreeMap::new(),
    )?;
    step.condition = Some(pr_cache::prep_condition(policy));
    Ok(step)
}

fn save_step(policy: PullRequestCachePolicy) -> Result<Step, RenderError> {
    let mut step = crate::steps::action_step(
        MBX_BUNDLE_SAVE_NAME,
        TOOLS_SAVE_USES,
        BTreeMap::from([
            (
                "key".to_owned(),
                "${{ steps.mbx-cache-key.outputs.key }}".to_owned(),
            ),
            ("path".to_owned(), MBX_BUNDLE_PATH.to_owned()),
        ]),
    )?;
    step.condition = Some(pr_cache::save_condition(policy));
    Ok(step)
}
