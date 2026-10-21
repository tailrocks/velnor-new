//! Opaque MBX bundle transport: key, restore, import, producer mark, export, save.
//!
//! Velnor archives only the bytes `mbx cache export --format directory`
//! writes and restores them only through `mbx cache import` (cache decision
//! 3). MBX owns the format; Velnor never parses or composes its objects. The
//! pinned native action is install-only (`backend: local`).
//!
//! Decision 1: the save is gated on the family's producer. The compile step
//! is followed by `Record MBX producer success`, which writes a marker file;
//! the export step runs `always()` on trusted pushes and exports only when
//! that marker exists, so a failing test, lint or doc step no longer starves
//! the cache, while a failed compile never saves. The marker lives in the
//! runner temp directory, so the gate also holds when the job body is a
//! shared composite action.

use std::collections::BTreeMap;

use velnor_actions_contract::workflow::cache_family::CacheFamily;
use velnor_actions_contract::workflow::cache_family_mbx::{
    MBX_BUNDLE_EXPORT_CONDITION, MBX_BUNDLE_IMPORT_CONDITION, MBX_BUNDLE_KEY_EXPR,
    MBX_BUNDLE_PATH_EXPR, MBX_BUNDLE_RESTORE_KEYS_EXPR, MBX_BUNDLE_SAVE_CONDITION,
};
use velnor_actions_contract::{Step, StepId, StepKind, StepRole};

use super::mbx_preflight::{ACTION_MBX_BIN, RESOLVE_ACTION_MBX_BIN};
use crate::RenderError;
use crate::steps::{action_owned_mbx_shell_step, action_step_with_env, shell_step};

/// Display name of the bundle key step.
pub const MBX_BUNDLE_KEY_NAME: &str = "Compute MBX bundle key";
/// Display name of the bundle restore step.
pub const MBX_BUNDLE_RESTORE_NAME: &str = "Restore MBX bundle";
/// Display name of the bundle import step.
pub const MBX_BUNDLE_IMPORT_NAME: &str = "Import MBX bundle";
/// Display name of the producer marker step.
pub const MBX_PRODUCER_MARK_NAME: &str = "Record MBX producer success";
/// Display name of the bundle export step.
pub const MBX_BUNDLE_EXPORT_NAME: &str = "Export MBX bundle";
/// Display name of the bundle save step.
pub const MBX_BUNDLE_SAVE_NAME: &str = "Save MBX bundle";
/// Job environment key naming the `mbx` export group of this job's builds.
pub const MBX_EXPORT_GROUP_ENV: &str = "MBX_CACHE_EXPORT_GROUP";
/// Job environment value of [`MBX_EXPORT_GROUP_ENV`] (one group per private runner job).
pub const MBX_EXPORT_GROUP_VALUE: &str = "velnor";

const DEFAULT_REF_ENV: &str = "VELNOR_DEFAULT_REF";
const HEAD_FORK_ENV: &str = "VELNOR_PR_HEAD_FORK";
const DEFAULT_REF_EXPR: &str =
    "${{ format('refs/heads/{0}', github.event.repository.default_branch) }}";
const HEAD_FORK_EXPR: &str = "${{ github.event.pull_request.head.repo.fork }}";
const VERSION_ENV: &str = "VELNOR_MBX_BUNDLE_VERSION";
const TOOLCHAIN_ENV: &str = "VELNOR_MBX_BUNDLE_TOOLCHAIN";
const ROOT_ENV: &str = "VELNOR_MBX_BUNDLE_ROOT";
const MARK_FILE: &str = "$RUNNER_TEMP/velnor/mbx-producer-ok";
const BUNDLE_SHELL_DIR: &str = "$RUNNER_TEMP/velnor/mbx-bundle";
const KIB: u64 = 1024;

const RUNNER_TEMP_GUARD: &str = "case \"$RUNNER_TEMP\" in /*) ;; *) printf '%s\\n' 'RUNNER_TEMP must be absolute' >&2; exit 1 ;; esac";

/// Workspace root relative to the checkout, or `.` for the checkout root.
fn workspace_root(working_directory: Option<&str>) -> Result<String, RenderError> {
    let Some(dir) = working_directory.filter(|dir| !dir.is_empty() && *dir != ".") else {
        return Ok(".".to_owned());
    };
    // Reuse the cleanup step's path grammar (relative, no traversal).
    crate::action_mbx_shell::target_dir_for_action(&BTreeMap::from([(
        "working-directory".to_owned(),
        dir.to_owned(),
    )]))?;
    Ok(dir.to_owned())
}

/// Bundle prelude: key, restore, import (inserted after the MBX ready check).
///
/// # Errors
///
/// Returns render errors for an invalid version, toolchain or workspace root.
pub(crate) fn mbx_bundle_prelude(
    unit: &str,
    mbx_version_expr: &str,
    toolchain_expr: &str,
    rust_toolchain: &str,
    working_directory: Option<&str>,
    env: &BTreeMap<String, String>,
) -> Result<[Step; 3], RenderError> {
    let root = workspace_root(working_directory)?;
    let key = key_step(unit, mbx_version_expr, toolchain_expr, &root)?;
    let restore = restore_step()?;
    let import = import_step(rust_toolchain, working_directory, env.clone())?;
    Ok([key, restore, import])
}

fn key_step(unit: &str, version: &str, toolchain: &str, root: &str) -> Result<Step, RenderError> {
    let spec = CacheFamily::MbxBundle.spec();
    let prefix = CacheFamily::MbxBundle.key_prefix();
    let ceiling = spec.ceiling_bytes;
    let script = [
        "set -eu".to_owned(),
        RUNNER_TEMP_GUARD.to_owned(),
        "d=\"$RUNNER_TEMP/velnor/mbx-bundle-key\"".to_owned(),
        "rm -rf \"$d\"".to_owned(),
        "mkdir -p \"$d\"".to_owned(),
        "s256() { sha256sum \"$1\" > \"$d/sum\"; h=; rest=; read -r h rest < \"$d/sum\" || [ -n \"$h\" ]; case \"$h\" in ''|*[!0-9a-f]*) printf '%s\\n' 'sha256sum returned no digest' >&2; exit 1 ;; esac; [ \"${#h}\" -eq 64 ] || exit 1; }".to_owned(),
        format!("default_ref=\"${{{DEFAULT_REF_ENV}:-}}\""),
        "trust=none".to_owned(),
        "if [ \"${GITHUB_EVENT_NAME:-}\" = push ] && [ -n \"${default_ref#refs/heads/}\" ] && [ \"${GITHUB_REF:-}\" = \"$default_ref\" ] && [ \"${GITHUB_REF_PROTECTED:-}\" = true ]; then trust=trusted; fi".to_owned(),
        format!("printf '%s\\n' 'family={}' 'schema={}' '{}' 'unit={unit}' \"mbx=${{{VERSION_ENV}}}\" \"toolchain=${{{TOOLCHAIN_ENV}}}\" \"root=${{{ROOT_ENV}}}\" 'ceiling={ceiling}' > \"$d/compat\"", spec.layer, spec.schema, spec.shell_paths(None).join("' '")),
        "uname -s >> \"$d/compat\"".to_owned(),
        "uname -m >> \"$d/compat\"".to_owned(),
        "s256 \"$d/compat\"".to_owned(),
        "compat=\"s256-$h\"".to_owned(),
        format!("lock=\"$GITHUB_WORKSPACE/${{{ROOT_ENV}}}/Cargo.lock\""),
        "if [ -f \"$lock\" ] && [ ! -L \"$lock\" ]; then s256 \"$lock\"; else printf '%s\\n' absent > \"$d/absent\"; s256 \"$d/absent\"; fi".to_owned(),
        "snap=\"s256-$h\"".to_owned(),
        format!("trusted=\"{prefix}trusted-$compat-\""),
        "key=\"$trusted$snap\"".to_owned(),
        format!("[ \"${{#key}}\" -le {} ] || {{ printf 'cache key exceeds the GitHub cap: %s\\n' \"$key\" >&2; exit 1; }}", velnor_actions_contract::cachekey::MAX_CACHE_KEY_BYTES),
        "save=false".to_owned(),
        "[ \"$trust\" != trusted ] || save=true".to_owned(),
        "printf '%s\\n' \"key=$key\" \"restore_keys=$trusted\" \"save=$save\" >> \"$GITHUB_OUTPUT\"".to_owned(),
        "printf 'velnor: mbx bundle trust=%s key=%s save=%s\\n' \"$trust\" \"$key\" \"$save\"".to_owned(),
    ]
    .join("; ");
    let env = BTreeMap::from([
        (DEFAULT_REF_ENV.to_owned(), DEFAULT_REF_EXPR.to_owned()),
        (HEAD_FORK_ENV.to_owned(), HEAD_FORK_EXPR.to_owned()),
        (VERSION_ENV.to_owned(), version.to_owned()),
        (TOOLCHAIN_ENV.to_owned(), toolchain.to_owned()),
        (ROOT_ENV.to_owned(), root.to_owned()),
    ]);
    let mut step = shell_step(
        MBX_BUNDLE_KEY_NAME,
        vec!["sh".to_owned(), "-c".to_owned(), script],
        env,
    )?;
    step.id = Some(StepId::MbxBundleKey);
    step.role = Some(StepRole::MbxBundleKey);
    Ok(step)
}

fn restore_step() -> Result<Step, RenderError> {
    let uses = crate::cache_steps::TOOLS_RESTORE_ACTION_USES;
    let mut step = action_step_with_env(
        MBX_BUNDLE_RESTORE_NAME,
        uses,
        BTreeMap::from([
            ("key".to_owned(), MBX_BUNDLE_KEY_EXPR.to_owned()),
            (
                "restore-keys".to_owned(),
                MBX_BUNDLE_RESTORE_KEYS_EXPR.to_owned(),
            ),
            ("path".to_owned(), MBX_BUNDLE_PATH_EXPR.to_owned()),
        ]),
        BTreeMap::new(),
    )?;
    step.id = Some(StepId::MbxBundleRestore);
    step.role = Some(StepRole::MbxBundleRestore);
    Ok(step)
}

/// `cd` into the Cargo workspace so `mbx` sees the matching checkout state.
fn workspace_cd(working_directory: Option<&str>) -> Result<String, RenderError> {
    let root = workspace_root(working_directory)?;
    Ok(if root == "." {
        "cd \"$GITHUB_WORKSPACE\"".to_owned()
    } else {
        format!("cd \"$GITHUB_WORKSPACE/{root}\"")
    })
}

/// Import the restored bundle; a bad bundle is discarded and the build runs cold.
pub(crate) fn import_step(
    rust_toolchain: &str,
    working_directory: Option<&str>,
    env: BTreeMap<String, String>,
) -> Result<Step, RenderError> {
    super::mbx_preflight::validate_rust_toolchain(rust_toolchain)?;
    let script = [
        "set -eu".to_owned(),
        RUNNER_TEMP_GUARD.to_owned(),
        format!("dir=\"{BUNDLE_SHELL_DIR}\""),
        "if [ ! -d \"$dir\" ] || [ -L \"$dir\" ]; then printf '%s\\n' 'velnor: mbx bundle no_entry'; exit 0; fi".to_owned(),
        RESOLVE_ACTION_MBX_BIN.to_owned(),
        workspace_cd(working_directory)?,
        format!("if mise --no-config --no-env --no-hooks exec rust@{rust_toolchain} -- \"{ACTION_MBX_BIN}\" cache import \"$dir\"; then printf '%s\\n' 'velnor: mbx bundle imported'; else printf '%s\\n' '::warning::cache_corrupt: MBX bundle import failed; building cold'; fi"),
        "rm -rf \"$dir\"".to_owned(),
    ]
    .join("; ");
    let mut step = action_owned_mbx_shell_step(
        MBX_BUNDLE_IMPORT_NAME,
        vec!["sh".to_owned(), "-c".to_owned(), script],
        env,
    )?;
    step.condition = Some(MBX_BUNDLE_IMPORT_CONDITION.to_owned());
    step.role = Some(StepRole::MbxBundleImport);
    Ok(step)
}

/// Marker step placed right after the compile (the family's producer).
///
/// It runs under the default `success()`, so the marker exists only when
/// every earlier step, including the compile, succeeded.
///
/// # Errors
///
/// Returns render errors for a rejected step shape.
pub fn mbx_producer_mark_step() -> Result<Step, RenderError> {
    let script = [
        "set -eu".to_owned(),
        RUNNER_TEMP_GUARD.to_owned(),
        "mkdir -p \"$RUNNER_TEMP/velnor\"".to_owned(),
        format!(": > \"{MARK_FILE}\""),
    ]
    .join("; ");
    let mut step = shell_step(
        MBX_PRODUCER_MARK_NAME,
        vec!["sh".to_owned(), "-c".to_owned(), script],
        BTreeMap::new(),
    )?;
    step.role = Some(StepRole::MbxProducerMark);
    Ok(step)
}

/// Export and save steps for one job, derived from its native MBX action.
pub(crate) fn bundle_postlude(action: &Step) -> Result<[Step; 2], RenderError> {
    let StepKind::Action { with, env, .. } = &action.kind else {
        return Err(RenderError::InvalidWorkflow(
            "mbx_bundle_action_kind_mismatch".to_owned(),
        ));
    };
    let rust = with
        .get("toolchain")
        .ok_or_else(|| RenderError::InvalidWorkflow("mbx_bundle_toolchain_missing".to_owned()))?;
    let working_directory = with.get("working-directory").map(String::as_str);
    let export = export_step(rust, working_directory, env.clone())?;
    let save = save_step()?;
    Ok([export, save])
}

/// `mbx cache export` gated on the producer marker and the family ceiling.
pub(crate) fn export_step(
    rust_toolchain: &str,
    working_directory: Option<&str>,
    env: BTreeMap<String, String>,
) -> Result<Step, RenderError> {
    super::mbx_preflight::validate_rust_toolchain(rust_toolchain)?;
    let ceiling_kib = CacheFamily::MbxBundle.spec().ceiling_bytes / KIB;
    let script = [
        "set -eu".to_owned(),
        RUNNER_TEMP_GUARD.to_owned(),
        format!("dir=\"{BUNDLE_SHELL_DIR}\""),
        format!("mark=\"{MARK_FILE}\""),
        "out=${GITHUB_OUTPUT-}".to_owned(),
        "skip() { if [ -n \"$out\" ]; then printf '%s\\n' 'exported=false' >> \"$out\"; fi; printf 'velnor: mbx bundle not exported (%s)\\n' \"$1\"; exit 0; }".to_owned(),
        "if [ ! -f \"$mark\" ] || [ -L \"$mark\" ]; then skip producer_not_succeeded; fi".to_owned(),
        format!("[ -n \"${{{MBX_EXPORT_GROUP_ENV}:-}}\" ] || skip export_group_missing"),
        RESOLVE_ACTION_MBX_BIN.to_owned(),
        workspace_cd(working_directory)?,
        "rm -rf \"$dir\"".to_owned(),
        format!("if mise --no-config --no-env --no-hooks exec rust@{rust_toolchain} -- \"{ACTION_MBX_BIN}\" cache export --group \"${MBX_EXPORT_GROUP_ENV}\" --format directory \"$dir\"; then :; else rm -rf \"$dir\"; printf '%s\\n' '::warning::cache_unavailable: MBX export failed or had nothing new'; skip export_failed; fi"),
        "if [ ! -d \"$dir\" ] || [ -L \"$dir\" ]; then skip export_missing; fi".to_owned(),
        "du -sk \"$dir\" > \"$RUNNER_TEMP/velnor/mbx-bundle.size\"".to_owned(),
        "kib=; rest=; read -r kib rest < \"$RUNNER_TEMP/velnor/mbx-bundle.size\" || [ -n \"$kib\" ]".to_owned(),
        "case \"$kib\" in ''|*[!0-9]*) rm -rf \"$dir\"; skip size_unreadable ;; esac".to_owned(),
        format!("if [ \"$kib\" -gt {ceiling_kib} ]; then rm -rf \"$dir\"; printf '%s\\n' '::warning::over_ceiling: MBX bundle exceeds the family ceiling; save refused'; skip over_ceiling; fi"),
        "if [ \"$kib\" -eq 0 ]; then rm -rf \"$dir\"; skip empty_export; fi".to_owned(),
        "if [ -n \"$out\" ]; then printf '%s\\n' 'exported=true' >> \"$out\"; fi".to_owned(),
        "printf 'velnor: mbx bundle exported (%s KiB)\\n' \"$kib\"".to_owned(),
    ]
    .join("; ");
    let mut step = action_owned_mbx_shell_step(
        MBX_BUNDLE_EXPORT_NAME,
        vec!["sh".to_owned(), "-c".to_owned(), script],
        env,
    )?;
    step.id = Some(StepId::MbxBundleExport);
    step.condition = Some(MBX_BUNDLE_EXPORT_CONDITION.to_owned());
    step.role = Some(StepRole::MbxBundleExport);
    Ok(step)
}

fn save_step() -> Result<Step, RenderError> {
    let mut step = action_step_with_env(
        MBX_BUNDLE_SAVE_NAME,
        crate::cache_steps::TOOLS_SAVE_USES,
        BTreeMap::from([
            ("key".to_owned(), MBX_BUNDLE_KEY_EXPR.to_owned()),
            ("path".to_owned(), MBX_BUNDLE_PATH_EXPR.to_owned()),
        ]),
        BTreeMap::new(),
    )?;
    step.condition = Some(MBX_BUNDLE_SAVE_CONDITION.to_owned());
    step.role = Some(StepRole::MbxBundleSave);
    Ok(step)
}

/// Append the export and save to every job that records a producer mark.
///
/// Runs right before the workspace cleanup is appended, so the cleanup stays
/// the final step. A job that restores a bundle but records no producer mark
/// is a reader (crate jobs restore the dependency seed) and gets neither.
///
/// # Errors
///
/// Returns render errors when a job carries a half-built bundle lifecycle.
pub(crate) fn append_bundle_postludes(
    jobs: &mut BTreeMap<String, velnor_actions_contract::Job>,
) -> Result<(), RenderError> {
    for (id, job) in jobs {
        let restores = count(&job.steps, StepRole::MbxBundleRestore);
        let exports = count(&job.steps, StepRole::MbxBundleExport);
        let marks = count(&job.steps, StepRole::MbxProducerMark);
        if restores == 0 && exports == 0 && marks == 0 {
            continue;
        }
        if marks == 0 && exports == 0 {
            // A reader: it restores the bundle and never writes one.
            continue;
        }
        if marks != 1 {
            return Err(RenderError::InvalidWorkflow(format!(
                "mbx_producer_mark_count:{id}"
            )));
        }
        if restores != 1 || exports != 0 {
            return Err(RenderError::InvalidWorkflow(format!(
                "mbx_bundle_lifecycle:{id}"
            )));
        }
        let action = job
            .steps
            .iter()
            .find(|step| step.role == Some(StepRole::MbxCache))
            .ok_or_else(|| RenderError::InvalidWorkflow(format!("mbx_bundle_no_action:{id}")))?;
        let postlude = bundle_postlude(action)?;
        job.steps.extend(postlude);
    }
    Ok(())
}

fn count(steps: &[Step], role: StepRole) -> usize {
    steps.iter().filter(|step| step.role == Some(role)).count()
}

#[cfg(test)]
#[path = "cache_steps_mbx_bundle_tests.rs"]
mod tests;
