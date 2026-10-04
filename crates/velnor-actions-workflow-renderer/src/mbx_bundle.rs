//! Explicit, job-private MBX directory-cache lifecycle.
//!
//! The official action runs with its supported `local` backend, so it only
//! installs the pinned MBX release. Velnor then restores/imports and exports/
//! saves one stable directory through pinned `actions/cache` steps. Each job
//! gets a fresh MBX root; failed imports abandon that root before compilation.
//! The official exporter replaces an existing bundle atomically, and MBX GC
//! reclaims only state under the fresh root. Export still copies objects, so
//! the workflow does not claim a lower peak disk use without hosted workload
//! measurements.

use std::collections::BTreeMap;

use velnor_actions_contract::workflow::lanes::{HOSTED_SUFFIX, SCALE_SUFFIX};
use velnor_actions_contract::{Job, PullRequestCachePolicy, Step, StepKind};

use crate::RenderError;
use crate::cache_steps::{
    MBX_ACTION_NAME, MBX_PREFLIGHT_NAME, TOOLS_RESTORE_USES, TOOLS_SAVE_USES, is_mbx_action,
    mbx_path_preflight_step,
};
#[path = "mbx_bundle_backend.rs"]
mod backend;
#[path = "mbx_bundle_identity.rs"]
mod identity;
#[path = "mbx_bundle_lane.rs"]
mod lane;
#[path = "mbx_bundle_observer.rs"]
mod observer;
#[path = "mbx_bundle_pr_cache.rs"]
mod pr_cache;
#[path = "mbx_bundle_qualification.rs"]
mod qualification;
use backend::{pin_local_backend, reject_private_env_overrides};
use identity::{CacheIdentity, import_guard, plan_writers};
pub(crate) use lane::{bind_shared_lane_outputs, shared_lane_policy};
pub(crate) use observer::qualification_observer_steps;
pub(crate) use pr_cache::QualificationObserverBinding;

pub(crate) const MBX_PR_CACHE_ALLOWED_OUTPUT: &str = pr_cache::PR_CACHE_ALLOWED_OUTPUT;

/// Display name of the fresh, private MBX root step.
pub(crate) const MBX_ROOT_NAME: &str = "Prepare private MBX store";
/// Display name of the reclaim-and-export step.
pub(crate) const MBX_BUNDLE_EXPORT_NAME: &str = "Export MBX single bundle";
/// Display name of the one-file cache save.
pub(crate) const MBX_BUNDLE_SAVE_NAME: &str = "Save MBX single bundle";
/// Display name of the key step ahead of the bundle restore.
pub(crate) const MBX_BUNDLE_KEY_NAME: &str = "Prepare MBX bundle key";
/// Display name of the bundle restore. Same path the save archived.
pub(crate) const MBX_BUNDLE_RESTORE_NAME: &str = "Restore MBX single bundle";
/// Display name of the import into the private MBX store.
pub(crate) const MBX_BUNDLE_IMPORT_NAME: &str = "Import MBX single bundle";
/// Bind export snapshots to the resource sampler actually attached to a job.
pub(crate) const MBX_RESOURCE_EVIDENCE_REQUIRED_ENV: &str =
    "MBX_QUALIFICATION_RESOURCE_EVIDENCE_REQUIRED";
/// Bundle path and cache action version stay stable across jobs and runs.
pub(crate) const MBX_BUNDLE_PATH: &str = "${{ runner.temp }}/mbx-single-bundle";
/// Internal-only MBX input that selects a shared logical cache scope.
const MBX_SCOPE_INPUT: &str = "velnor-cache-scope";
/// Internal-only MBX input that marks an intentional cache reader/writer.
const MBX_WRITER_INPUT: &str = "velnor-cache-writer";
/// Reserved namespace keeps qualification bundles apart from job caches.
const MBX_QUALIFICATION_SCOPE_PREFIX: &str = "qualification-mbx-v1/";
const SHARED_CACHE_KEY: &str = "${{ steps.mbx-lane-cache.outputs.mbx-cache-key }}";

/// Create one exclusive MBX cache root before any MBX command runs.
const ROOT_SCRIPT: &str = r#"set -eu; test -n "$RUNNER_TEMP"; bundle="$RUNNER_TEMP/mbx-single-bundle"; if [ -e "$bundle" ] || [ -L "$bundle" ]; then echo "stable MBX bundle path already exists in runner.temp" >&2; exit 1; fi; root_file="$GITHUB_OUTPUT.mbx-root"; if [ -e "$root_file" ] || [ -L "$root_file" ]; then echo "MBX root marker already exists" >&2; exit 1; fi; mktemp -d "$RUNNER_TEMP/velnor-mbx-store.XXXXXXXXXX" > "$root_file"; IFS= read -r root < "$root_file"; root_id="${root##*/}"; group="velnor-${GITHUB_RUN_ID}-${GITHUB_RUN_ATTEMPT}-${root_id}"; printf 'MBX_CACHE_DIR=%s\nMBX_TARGET_ROOT=%s/targets\nMBX_SHIMS_DIR=%s/shims\nMBX_CACHE_EXPORT_GROUP=%s\n' "$root" "$root" "$root" "$group" >> "$GITHUB_ENV""#;

/// Export first, then ask MBX's lock-aware collector to reclaim its private root.
const EXPORT_SCRIPT: &str = r#"set -eu; df -B1 -P "$RUNNER_TEMP"; df -i -P "$RUNNER_TEMP"; bundle="$RUNNER_TEMP/mbx-single-bundle"; export_log="$GITHUB_OUTPUT.mbx-export"; gc_log="$GITHUB_OUTPUT.mbx-gc"; for file in "$export_log" "$gc_log"; do if [ -e "$file" ] || [ -L "$file" ]; then echo "MBX export marker already exists" >&2; exit 1; fi; done; if mbx cache export --group "$MBX_CACHE_EXPORT_GROUP" --format directory "$bundle" >"$export_log" 2>&1; then test -d "$bundle"; df -B1 -P "$RUNNER_TEMP"; df -i -P "$RUNNER_TEMP"; if mbx gc --max-size 0 --json >"$gc_log" 2>&1; then cat "$gc_log"; echo 'gc-succeeded=true' >> "$GITHUB_OUTPUT"; echo 'ready=true' >> "$GITHUB_OUTPUT"; else cat "$gc_log"; echo 'gc-succeeded=false' >> "$GITHUB_OUTPUT"; echo 'ready=false' >> "$GITHUB_OUTPUT"; fi; df -B1 -P "$RUNNER_TEMP"; df -i -P "$RUNNER_TEMP"; else if grep -Fq 'no completed mbx builds are recorded for export group' "$export_log"; then cat "$export_log"; echo 'ready=false' >> "$GITHUB_OUTPUT"; exit 0; fi; cat "$export_log"; exit 1; fi"#;

/// Failed imports never keep compiling against a partially published store.
const IMPORT_SCRIPT: &str = r#"set -eu; bundle="$RUNNER_TEMP/mbx-single-bundle"; df -B1 -P "$RUNNER_TEMP"; df -i -P "$RUNNER_TEMP"; if [ -z "$MATCHED" ]; then echo 'no MBX bundle matched'; printf 'selected_cache_root=%s\n' "$MBX_CACHE_DIR" >> "$GITHUB_OUTPUT"; exit 0; fi; if [ ! -d "$bundle" ]; then echo 'matched MBX bundle is missing; the fresh store stays cold'; printf 'selected_cache_root=%s\n' "$MBX_CACHE_DIR" >> "$GITHUB_OUTPUT"; exit 0; fi; if mbx cache import "$bundle"; then echo 'MBX bundle imported'; printf 'selected_cache_root=%s\n' "$MBX_CACHE_DIR" >> "$GITHUB_OUTPUT"; else echo 'MBX bundle import failed; abandoning its private store'; root_file="$GITHUB_OUTPUT.mbx-fallback"; if [ -e "$root_file" ] || [ -L "$root_file" ]; then echo "MBX fallback marker already exists" >&2; exit 1; fi; mktemp -d "$RUNNER_TEMP/velnor-mbx-fallback.XXXXXXXXXX" > "$root_file"; IFS= read -r root < "$root_file"; root_id="${root##*/}"; group="velnor-${GITHUB_RUN_ID}-${GITHUB_RUN_ATTEMPT}-${root_id}"; printf 'MBX_CACHE_DIR=%s\nMBX_TARGET_ROOT=%s/targets\nMBX_SHIMS_DIR=%s/shims\nMBX_CACHE_EXPORT_GROUP=%s\n' "$root" "$root" "$root" "$group" >> "$GITHUB_ENV"; printf 'selected_cache_root=%s\n' "$root" >> "$GITHUB_OUTPUT"; echo 'fresh cold MBX store selected for subsequent steps'; fi"#;
/// YAML step id for MBX setup and the key/export steps that expose outputs.
pub(crate) fn step_yaml_id(step: &Step) -> Option<&'static str> {
    if is_mbx_action(step) {
        Some("mbx")
    } else {
        match step.name.as_str() {
            MBX_BUNDLE_KEY_NAME => Some("mbx-bundle-key"),
            MBX_BUNDLE_RESTORE_NAME => Some("mbx-bundle"),
            MBX_BUNDLE_IMPORT_NAME => Some("mbx-bundle-import"),
            MBX_BUNDLE_EXPORT_NAME => Some("mbx-export"),
            MBX_BUNDLE_SAVE_NAME if qualification_save_id(step) => Some("mbx-bundle-save"),
            _ => None,
        }
    }
}

fn qualification_save_id(step: &Step) -> bool {
    matches!(&step.kind, StepKind::Action { env, .. } if env.contains_key("VELNOR_QUALIFICATION_SAVE_STEP_ID"))
}

/// Emit `id:` for steps whose later steps read outputs.
pub(crate) fn push_step_id(entries: &mut Vec<(String, crate::yaml::Yaml)>, step: &Step) {
    let Some(id) = step_yaml_id(step) else {
        return;
    };
    entries.push(("id".to_owned(), crate::yaml::Yaml::str(id.to_owned())));
}

/// Use the supported local backend and append one explicit bundle lifecycle.
///
/// The runtime key includes the installed MBX version, directory format,
/// runner OS/architecture, rustc identity, and commit. Jobs with the same
/// static runner/version/toolchain selector elect one deterministic writer.
///
/// # Errors
///
/// Returns [`RenderError`] when a job overrides the private MBX store/group
/// environment or when a generated restore/export/save step is invalid.
pub(crate) fn append_single_bundle_saves(
    jobs: &mut BTreeMap<String, Job>,
    pull_request_cache_policy: PullRequestCachePolicy,
) -> Result<(), RenderError> {
    let (identities, winners) = plan_writers(jobs)?;
    for (id, job) in jobs.iter_mut() {
        if !job.steps.iter().any(is_mbx_action) {
            continue;
        }
        let identity = identities.get(id).ok_or_else(|| {
            RenderError::InvalidWorkflow(format!("mbx_cache_identity_missing:{id}"))
        })?;
        let cache_policy = identity.effective_policy(pull_request_cache_policy);
        reject_private_env_overrides(job, id)?;
        pin_local_backend(job, id)?;
        insert_root_step(job)?;
        insert_mbx_preflight_step(job, id, identity)?;
        insert_bundle_restore(job, identity, cache_policy)?;
        if winners.contains(id) {
            insert_writer_steps(job, identity.qualification_role(), cache_policy)?;
        } else if has_writer_steps(job) {
            return Err(RenderError::InvalidWorkflow(format!(
                "mbx_non_elected_writer:{id}"
            )));
        }
    }
    Ok(())
}

/// Insert the centrally owned tool check immediately ahead of the private root.
fn insert_mbx_preflight_step(
    job: &mut Job,
    id: &str,
    identity: &CacheIdentity,
) -> Result<(), RenderError> {
    let Some(root_index) = job.steps.iter().position(|step| step.name == MBX_ROOT_NAME) else {
        return Err(RenderError::InvalidWorkflow(format!(
            "mbx_private_root_missing:{id}"
        )));
    };
    let preflight = mbx_path_preflight_step(
        &identity.version,
        &identity.rustup_toolchain,
        identity.rust_env.clone(),
    )?;
    let preflight_positions: Vec<usize> = job
        .steps
        .iter()
        .enumerate()
        .filter_map(|(index, step)| (step.name == MBX_PREFLIGHT_NAME).then_some(index))
        .collect();
    match preflight_positions.as_slice() {
        [] => {}
        [index] if *index + 1 == root_index && job.steps.get(*index) == Some(&preflight) => {
            return Ok(());
        }
        _ => {
            return Err(RenderError::InvalidWorkflow(format!(
                "mbx_preflight_mismatch:{id}"
            )));
        }
    }
    job.steps.insert(root_index, preflight);
    Ok(())
}

/// Add a fresh MBX cache root before the earliest MBX action or command.
fn insert_root_step(job: &mut Job) -> Result<(), RenderError> {
    if job.steps.iter().any(|step| step.name == MBX_ROOT_NAME) {
        return Ok(());
    }
    let Some(index) = job.steps.iter().position(|step| {
        is_mbx_action(step)
            || matches!(&step.kind, StepKind::Shell { run, .. } if run.iter().any(|arg| arg == "mbx"))
    }) else {
        return Ok(());
    };
    let root = crate::steps::shell_step(
        MBX_ROOT_NAME,
        vec!["bash".to_owned(), "-c".to_owned(), ROOT_SCRIPT.to_owned()],
        BTreeMap::new(),
    )?;
    job.steps.insert(index, root);
    Ok(())
}

/// Insert runtime key, restore, and import immediately after local setup.
fn insert_bundle_restore(
    job: &mut Job,
    identity: &CacheIdentity,
    pull_request_cache_policy: PullRequestCachePolicy,
) -> Result<(), RenderError> {
    if job
        .steps
        .iter()
        .any(|step| step.name == MBX_BUNDLE_RESTORE_NAME)
    {
        return Ok(());
    }
    let Some(index) = job.steps.iter().enumerate().find_map(|(index, step)| {
        let StepKind::Action { uses, with, .. } = &step.kind else {
            return None;
        };
        if !uses.starts_with(&format!("{MBX_ACTION_NAME}@")) {
            return None;
        }
        with.get("version")?;
        Some(index)
    }) else {
        return Ok(());
    };
    let qualification_role = identity.qualification_role();
    let added = [
        pr_cache::key_step(
            MBX_BUNDLE_KEY_NAME,
            &identity.generation,
            &identity.version,
            &identity.scope,
            &identity.rust_env,
            qualification_role.is_some(),
            pull_request_cache_policy,
        )?,
        restore_step(identity.qualification_run_scoped())?,
        import_step(qualification_role)?,
    ];
    let insert_at = index + 1;
    for step in added.into_iter().rev() {
        job.steps.insert(insert_at, step);
    }
    Ok(())
}

fn insert_writer_steps(
    job: &mut Job,
    qualification_role: Option<bool>,
    pull_request_cache_policy: PullRequestCachePolicy,
) -> Result<(), RenderError> {
    if has_writer_steps(job) {
        return Ok(());
    }
    job.steps.push(export_step(
        qualification_role == Some(true),
        pull_request_cache_policy,
    )?);
    job.steps
        .push(save_step(qualification_role, pull_request_cache_policy)?);
    Ok(())
}

fn has_writer_steps(job: &Job) -> bool {
    job.steps
        .iter()
        .any(|step| step.name == MBX_BUNDLE_EXPORT_NAME || step.name == MBX_BUNDLE_SAVE_NAME)
}

fn restore_step(exact_only: bool) -> Result<Step, RenderError> {
    let mut with = BTreeMap::from([
        (
            "key".to_owned(),
            "${{ steps.mbx-bundle-key.outputs.primary }}".to_owned(),
        ),
        ("path".to_owned(), MBX_BUNDLE_PATH.to_owned()),
    ]);
    if !exact_only {
        with.insert(
            "restore-keys".to_owned(),
            "${{ steps.mbx-bundle-key.outputs.prefix }}".to_owned(),
        );
    }
    crate::steps::action_step(MBX_BUNDLE_RESTORE_NAME, TOOLS_RESTORE_USES, with)
}

fn import_step(qualification_role: Option<bool>) -> Result<Step, RenderError> {
    let mut env = BTreeMap::from([(
        "MATCHED".to_owned(),
        "${{ steps.mbx-bundle.outputs.cache-matched-key }}".to_owned(),
    )]);
    let guard = import_guard(qualification_role, &mut env);
    let script = qualification::import_script(qualification_role.is_some(), guard, IMPORT_SCRIPT);
    crate::steps::shell_step(
        MBX_BUNDLE_IMPORT_NAME,
        vec!["bash".to_owned(), "-c".to_owned(), script],
        env,
    )
}

fn export_step(
    qualification_writer: bool,
    pull_request_cache_policy: PullRequestCachePolicy,
) -> Result<Step, RenderError> {
    let script = qualification::export_script(qualification_writer, EXPORT_SCRIPT);
    let env = if qualification_writer {
        BTreeMap::from([(
            MBX_RESOURCE_EVIDENCE_REQUIRED_ENV.to_owned(),
            "false".to_owned(),
        )])
    } else {
        BTreeMap::new()
    };
    let mut step = crate::steps::shell_step(
        MBX_BUNDLE_EXPORT_NAME,
        vec!["bash".to_owned(), "-c".to_owned(), script],
        env,
    )?;
    step.condition = Some(pr_cache::prep_condition(pull_request_cache_policy));
    Ok(step)
}

fn save_step(
    qualification_role: Option<bool>,
    pull_request_cache_policy: PullRequestCachePolicy,
) -> Result<Step, RenderError> {
    let mut step = crate::steps::action_step(
        MBX_BUNDLE_SAVE_NAME,
        TOOLS_SAVE_USES,
        BTreeMap::from([
            (
                "key".to_owned(),
                "${{ steps.mbx-bundle-key.outputs.primary }}".to_owned(),
            ),
            ("path".to_owned(), MBX_BUNDLE_PATH.to_owned()),
        ]),
    )?;
    if qualification_role == Some(true)
        && let StepKind::Action { env, .. } = &mut step.kind
    {
        env.insert(
            "VELNOR_QUALIFICATION_SAVE_STEP_ID".to_owned(),
            "true".to_owned(),
        );
    }
    step.condition = Some(pr_cache::save_condition(pull_request_cache_policy));
    Ok(step)
}

#[cfg(test)]
#[path = "mbx_bundle_import_tests.rs"]
mod import_tests;
#[cfg(test)]
#[path = "mbx_bundle_observer_tests.rs"]
mod observer_tests;
