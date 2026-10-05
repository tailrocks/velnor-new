//! One stock-MBX local store with an explicit Actions cache bundle lifecycle.

use std::collections::BTreeMap;

use velnor_actions_contract::{Job, RunsOn, Step, StepKind};

use crate::cache_steps::{
    MBX_ACTION_NAME, MBX_CACHE_MODE_ENV, MBX_LOCAL_SETUP_NAME, MBX_SHARE_OUT_DIR_ENV,
    MBX_SHARE_OUT_DIR_VALUE, TOOLS_RESTORE_USES, TOOLS_SAVE_USES, is_mbx_action,
};
use crate::stock_mbx_bundle::{MbxBundleCacheSteps, lifecycle_steps};
use crate::{
    RenderError,
    steps::{shell_step, validate_uses},
};

/// Display name of the key-identity step.
pub(crate) const MBX_CACHE_KEY_NAME: &str = "Prepare MBX cache identity";
/// Display name of the restore action. The name is also its output ID source.
pub(crate) const MBX_BUNDLE_RESTORE_NAME: &str = "Restore MBX single bundle";
/// Display name of the import step.
pub(crate) const MBX_BUNDLE_IMPORT_NAME: &str = "Import MBX single bundle";
/// Display name of the export step.
pub(crate) const MBX_BUNDLE_EXPORT_NAME: &str = "Export MBX single bundle";
/// Display name of the save action.
pub(crate) const MBX_BUNDLE_SAVE_NAME: &str = "Save MBX single bundle";
/// Display name of the post-job cleanup step.
pub(crate) const MBX_BUNDLE_CLEANUP_NAME: &str = "Cleanup MBX single bundle";

const TRUSTED_PUSH: &str = "success() && github.event_name == 'push' && toJSON(github.event.repository.fork) == 'false' && github.event.repository.default_branch != '' && github.ref == format('refs/heads/{0}', github.event.repository.default_branch) && github.ref_protected == true";
const KEY_SCRIPT: &str = r#"set -eu
case "$RUNNER_OS:$RUNNER_ARCH" in Linux:X64|Linux:ARM64|macOS:X64|macOS:ARM64) ;; *) printf 'unsupported MBX runner %s/%s\n' "$RUNNER_OS" "$RUNNER_ARCH" >&2; exit 1 ;; esac
case "$GITHUB_REPOSITORY_ID" in ''|*[!0-9]*) printf '%s\n' 'invalid repository ID' >&2; exit 1 ;; esac
case "$GITHUB_JOB" in ''|*[!a-zA-Z0-9_-]*) printf '%s\n' 'invalid GitHub job ID' >&2; exit 1 ;; esac
case "$GITHUB_SHA" in *[!a-f0-9]*|'') printf '%s\n' 'invalid source revision' >&2; exit 1 ;; esac
[ "${#GITHUB_SHA}" -eq 40 ] || { printf '%s\n' 'source revision must be a full commit SHA' >&2; exit 1; }
case "$RUNNER_LANE" in ''|*[!a-zA-Z0-9_.-]*) printf '%s\n' 'invalid MBX runner lane' >&2; exit 1 ;; esac
case "$CACHE_GENERATION" in ''|*[!a-zA-Z0-9_.-]*) printf '%s\n' 'invalid MBX cache generation' >&2; exit 1 ;; esac
case "$RUST_TOOLCHAIN" in ''|*[!a-zA-Z0-9_.-]*) printf '%s\n' 'invalid Rust toolchain' >&2; exit 1 ;; esac
base="velnor-mbx-directory-v1-trusted-repo-${GITHUB_REPOSITORY_ID}-${RUNNER_LANE}-${RUNNER_OS}-${RUNNER_ARCH}-${CACHE_GENERATION}-format-directory-v1-rust-${RUST_TOOLCHAIN}-job-${GITHUB_JOB}"
key="${base}-sha-${GITHUB_SHA}"
[ "${#key}" -le 512 ] || { printf '%s\n' 'MBX cache key exceeds Actions limit' >&2; exit 1; }
printf 'key=%s\nprefix=%s-\n' "$key" "$base" >> "$GITHUB_OUTPUT""#;

/// Add one explicit restore/import before producers and export/save after them.
///
/// Every MBX job uses the action only for verified local setup. All jobs share
/// the same typed external bundle lifecycle, keyed by the complete runtime,
/// toolchain, runner lane, repository, and job namespace.
/// # Errors
pub(crate) fn append_single_bundle_saves(
    jobs: &mut BTreeMap<String, Job>,
) -> Result<(), RenderError> {
    for (job_id, job) in jobs {
        let action_indexes: Vec<_> = job
            .steps
            .iter()
            .enumerate()
            .filter_map(|(index, step)| is_mbx_action(step).then_some(index))
            .collect();
        if action_indexes.is_empty() {
            continue;
        }
        if action_indexes.len() != 1 {
            return Err(RenderError::InvalidWorkflow(format!(
                "mbx_action_count:{job_id}:{}",
                action_indexes.len()
            )));
        }
        if has_bundle_steps(&job.steps) {
            return Err(RenderError::InvalidWorkflow(format!(
                "mbx_bundle_steps_preexist:{job_id}"
            )));
        }
        let action_index = action_indexes[0];
        let key_step = prepare_key_step(job, &job.steps[action_index])?;
        job.steps.insert(action_index, key_step);
        let action_index = action_index + 1;
        configure_local_action(&mut job.steps[action_index])?;
        let lifecycle = bundle_lifecycle()?;
        job.steps
            .splice(action_index + 1..=action_index, lifecycle.restore);
        job.steps.extend(lifecycle.save);
    }
    Ok(())
}

fn has_bundle_steps(steps: &[Step]) -> bool {
    steps.iter().any(|step| {
        matches!(
            step.name.as_str(),
            MBX_CACHE_KEY_NAME
                | MBX_BUNDLE_RESTORE_NAME
                | MBX_BUNDLE_IMPORT_NAME
                | MBX_BUNDLE_EXPORT_NAME
                | MBX_BUNDLE_SAVE_NAME
                | MBX_BUNDLE_CLEANUP_NAME
        )
    })
}

fn configure_local_action(step: &mut Step) -> Result<(), RenderError> {
    let StepKind::Action { uses, with, env } = &mut step.kind else {
        return Err(RenderError::InvalidWorkflow(
            "mbx_setup_step_not_action".to_owned(),
        ));
    };
    if !uses.starts_with(&format!("{MBX_ACTION_NAME}@")) {
        return Err(RenderError::InvalidWorkflow(
            "mbx_setup_step_wrong_action".to_owned(),
        ));
    }
    let version = env
        .get("VELNOR_MBX_VERSION")
        .ok_or_else(|| RenderError::InvalidWorkflow("mbx_setup_version_missing".to_owned()))?;
    if with.get("backend").map(String::as_str) != Some("local")
        || with.get("github-cache-mode").map(String::as_str) != Some("objects")
        || with.get("version") != Some(version)
        || env.get(MBX_CACHE_MODE_ENV).map(String::as_str) != Some("read")
        || env.get(MBX_SHARE_OUT_DIR_ENV).map(String::as_str) != Some(MBX_SHARE_OUT_DIR_VALUE)
        || with.contains_key("cache-key")
        || with.contains_key("restore-keys")
    {
        return Err(RenderError::InvalidWorkflow(
            "mbx_local_setup_has_implicit_cache".to_owned(),
        ));
    }
    MBX_LOCAL_SETUP_NAME.clone_into(&mut step.name);
    Ok(())
}

fn prepare_key_step(job: &Job, action: &Step) -> Result<Step, RenderError> {
    let StepKind::Action { uses, with, .. } = &action.kind else {
        return Err(RenderError::InvalidWorkflow(
            "mbx_setup_step_not_action".to_owned(),
        ));
    };
    validate_uses(uses)?;
    let action_sha = uses
        .strip_prefix(&format!("{MBX_ACTION_NAME}@"))
        .ok_or_else(|| RenderError::BadActionRef("mbx_action_pin_missing".to_owned()))?;
    let generation = with
        .get("cache-generation")
        .cloned()
        .ok_or_else(|| RenderError::InvalidWorkflow("mbx_cache_generation_missing".to_owned()))?;
    let toolchain = with
        .get("toolchain")
        .ok_or_else(|| RenderError::InvalidWorkflow("mbx_toolchain_missing".to_owned()))?;
    let lane = lane_key(&job.runs_on)?;
    if !generation.ends_with(action_sha) {
        return Err(RenderError::InvalidWorkflow(
            "mbx_cache_generation_action_mismatch".to_owned(),
        ));
    }
    let env = BTreeMap::from([
        ("RUNNER_OS".to_owned(), "${{ runner.os }}".to_owned()),
        ("RUNNER_ARCH".to_owned(), "${{ runner.arch }}".to_owned()),
        (
            "GITHUB_REPOSITORY_ID".to_owned(),
            "${{ github.repository_id }}".to_owned(),
        ),
        ("GITHUB_JOB".to_owned(), "${{ github.job }}".to_owned()),
        ("GITHUB_SHA".to_owned(), "${{ github.sha }}".to_owned()),
        ("RUNNER_LANE".to_owned(), lane),
        ("CACHE_GENERATION".to_owned(), generation),
        ("RUST_TOOLCHAIN".to_owned(), toolchain.clone()),
    ]);
    let key_script = KEY_SCRIPT.replace('\n', "; ");
    shell_step(
        MBX_CACHE_KEY_NAME,
        vec!["sh".to_owned(), "-c".to_owned(), key_script],
        env,
    )
}

fn lane_key(runs_on: &str) -> Result<String, RenderError> {
    match RunsOn::parse(runs_on).map_err(RenderError::Contract)? {
        RunsOn::Hosted(label) => Ok(format!("hosted-{label}")),
        RunsOn::ScaleSet(selector) => Ok(format!("scale-set-{}", selector.name())),
    }
}

fn bundle_lifecycle() -> Result<MbxBundleCacheSteps, RenderError> {
    lifecycle_steps(
        TOOLS_RESTORE_USES,
        TOOLS_SAVE_USES,
        "${{ steps.mbx-cache-key.outputs.key }}",
        "${{ steps.mbx-cache-key.outputs.prefix }}",
        TRUSTED_PUSH,
    )
}

/// Serialize output IDs used by the restore/import/export helper steps.
pub(crate) fn step_yaml_id(name: &str) -> Option<&'static str> {
    match name {
        crate::cache_steps::MBX_PREFLIGHT_NAME => Some("mbx_cache_preflight"),
        MBX_CACHE_KEY_NAME => Some("mbx-cache-key"),
        MBX_LOCAL_SETUP_NAME => Some("mbx"),
        MBX_BUNDLE_RESTORE_NAME => Some("mbx_cache_restore"),
        MBX_BUNDLE_IMPORT_NAME => Some("mbx_cache_import"),
        MBX_BUNDLE_EXPORT_NAME => Some("mbx_cache_export"),
        MBX_BUNDLE_SAVE_NAME => Some("mbx_cache_save"),
        _ => None,
    }
}

/// Emit fixed IDs for generic generated MBX steps that later steps consume.
pub(crate) fn push_step_id(entries: &mut Vec<(String, crate::yaml::Yaml)>, name: &str) {
    if let Some(id) = step_yaml_id(name) {
        entries.push(("id".to_owned(), crate::yaml::Yaml::str(id.to_owned())));
    }
}
