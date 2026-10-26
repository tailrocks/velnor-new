// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use velnor_actions_contract::workflow::cache_family::{
    COMPLETE_STATE_ROOT_EXPR, COMPLETE_STATE_TRANSPORT_EXPR, CacheFamily, RUNNER_TEMP_EXPR,
    RUNNER_TEMP_SHELL,
};
use velnor_actions_contract::workflow::cache_family_mbx::{
    MBX_BUNDLE_EXPORT_CONDITION, MBX_BUNDLE_IMPORT_CONDITION, MBX_BUNDLE_KEY_EXPR,
    MBX_BUNDLE_RESTORE_KEYS_EXPR, MBX_BUNDLE_SAVE_CONDITION,
};
use velnor_actions_contract::{Step, StepId, StepKind, StepRole};

use crate::RenderError;
use crate::steps::shell_step;

pub const MBX_BUNDLE_KEY_NAME: &str = "Compute state snapshot key";
pub const MBX_BUNDLE_RESTORE_NAME: &str = "Restore state snapshot";
pub const MBX_BUNDLE_IMPORT_NAME: &str = "Recover state snapshot";
pub const MBX_PRODUCER_MARK_NAME: &str = "Record state producer success";
pub const MBX_BUNDLE_EXPORT_NAME: &str = "Publish state snapshot";
pub const MBX_BUNDLE_SAVE_NAME: &str = "Save state snapshot";
pub const MBX_EXPORT_GROUP_ENV: &str = "MBX_CACHE_EXPORT_GROUP";
pub const MBX_EXPORT_GROUP_VALUE: &str = "velnor";

const DEFAULT_REF_ENV: &str = "VELNOR_DEFAULT_REF";
const HEAD_FORK_ENV: &str = "VELNOR_PR_HEAD_FORK";
const DEFAULT_REF_EXPR: &str =
    "${{ format('refs/heads/{0}', github.event.repository.default_branch) }}";
const HEAD_FORK_EXPR: &str = "${{ github.event.pull_request.head.repo.fork }}";
const VERSION_ENV: &str = "VELNOR_MBX_BUNDLE_VERSION";
const TOOLCHAIN_ENV: &str = "VELNOR_MBX_BUNDLE_TOOLCHAIN";
const ROOT_ENV: &str = "VELNOR_MBX_BUNDLE_ROOT";
const MARK_FILE: &str = "$RUNNER_TEMP/velnor/state-producer-ok";
const SIZE_FILE: &str = "$RUNNER_TEMP/velnor/state-payload.size";

const RUNNER_TEMP_GUARD: &str = "case \"$RUNNER_TEMP\" in /*) ;; *) printf '%s\\n' 'RUNNER_TEMP must be absolute' >&2; exit 1 ;; esac";

fn workspace_root(working_directory: Option<&str>) -> Result<String, RenderError> {
    let Some(dir) = working_directory.filter(|dir| !dir.is_empty() && *dir != ".") else {
        return Ok(".".to_owned());
    };
    crate::action_mbx_shell::target_dir_for_action(&BTreeMap::from([(
        "working-directory".to_owned(),
        dir.to_owned(),
    )]))?;
    Ok(dir.to_owned())
}

/// Fixed shell literal for one complete-state transport expression.
///
/// Cache action inputs carry the expression form while shell scripts need
/// the shell form. Both derive from the same common family literal so the
/// transport path cannot drift between restore, recovery, publish, and save.
fn shell_literal(expr: &str) -> Result<String, RenderError> {
    expr.strip_prefix(RUNNER_TEMP_EXPR)
        .map(|rest| format!("{RUNNER_TEMP_SHELL}{rest}"))
        .ok_or_else(|| RenderError::InvalidWorkflow("state_transport_not_runner_temp".to_owned()))
}

fn split_shell(path: &str) -> Result<(String, String), RenderError> {
    path.rsplit_once('/')
        .map(|(parent, base)| (parent.to_owned(), base.to_owned()))
        .filter(|(parent, base)| !parent.is_empty() && !base.is_empty())
        .ok_or_else(|| RenderError::InvalidWorkflow("state_transport_bad_path".to_owned()))
}

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
        "d=\"$RUNNER_TEMP/velnor/state-snapshot-key\"".to_owned(),
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
        "printf 'velnor: state snapshot trust=%s key=%s save=%s\\n' \"$trust\" \"$key\" \"$save\"".to_owned(),
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
    let mut step = super::cache_action_step(
        true,
        crate::cache_steps::TOOLS_RESTORE_ACTION_USES,
        "mbx-bundle",
        MBX_BUNDLE_KEY_EXPR,
        &[MBX_BUNDLE_RESTORE_KEYS_EXPR.to_owned()],
        &[COMPLETE_STATE_TRANSPORT_EXPR.to_owned()],
    )?;
    MBX_BUNDLE_RESTORE_NAME.clone_into(&mut step.name);
    step.id = Some(StepId::MbxBundleRestore);
    step.role = Some(StepRole::MbxBundleRestore);
    Ok(step)
}

/// Recover the complete-state snapshot from the restored transport payload.
///
/// This is a common snapshot operation over fixed literals: unpack the
/// frozen payload into the fixed state root. It never invokes the build
/// tool and never selects an action ref; policy arrives through the typed
/// key and cache-hit outputs consumed by the step condition.
pub(crate) fn import_step(
    rust_toolchain: &str,
    _working_directory: Option<&str>,
    env: BTreeMap<String, String>,
) -> Result<Step, RenderError> {
    super::mbx_preflight::validate_rust_toolchain(rust_toolchain)?;
    let payload = shell_literal(COMPLETE_STATE_TRANSPORT_EXPR)?;
    let root = shell_literal(COMPLETE_STATE_ROOT_EXPR)?;
    let (parent, _base) = split_shell(&root)?;
    let script = [
        "set -eu".to_owned(),
        RUNNER_TEMP_GUARD.to_owned(),
        format!("payload=\"{payload}\""),
        format!("root=\"{root}\""),
        format!("parent=\"{parent}\""),
        "if [ ! -f \"$payload\" ] || [ -L \"$payload\" ]; then printf '%s\\n' 'velnor: state snapshot no_entry'; exit 0; fi".to_owned(),
        "if [ ! -s \"$payload\" ]; then printf '%s\\n' '::warning::cache_corrupt: state payload empty; building cold'; rm -f \"$payload\"; exit 0; fi".to_owned(),
        "if [ -L \"$root\" ]; then printf '%s\\n' '::warning::cache_corrupt: state root is a symlink; building cold'; exit 0; fi".to_owned(),
        "rm -rf \"$root\"".to_owned(),
        "mkdir -p \"$parent\"".to_owned(),
        "if tar -xzf \"$payload\" -C \"$parent\"; then if [ -d \"$root\" ] && [ ! -L \"$root\" ]; then printf '%s\\n' 'velnor: state snapshot recovered'; else printf '%s\\n' '::warning::cache_corrupt: state payload missed the state root; building cold'; rm -rf \"$root\"; rm -f \"$payload\"; fi; else printf '%s\\n' '::warning::cache_corrupt: state payload unreadable; building cold'; rm -rf \"$root\"; rm -f \"$payload\"; fi".to_owned(),
    ]
    .join("; ");
    let mut step = shell_step(
        MBX_BUNDLE_IMPORT_NAME,
        vec!["sh".to_owned(), "-c".to_owned(), script],
        env,
    )?;
    step.condition = Some(MBX_BUNDLE_IMPORT_CONDITION.to_owned());
    step.role = None;
    Ok(step)
}

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

/// Publish the complete-state snapshot into the frozen transport payload.
///
/// This is a common snapshot operation over fixed literals: pack the fixed
/// state root into the frozen payload and admit the exact payload bytes
/// against the family ceiling. The admission decision flows through the
/// typed `exported` output consumed by the save condition; the payload is
/// never republished when admission refuses it.
pub(crate) fn export_step(
    rust_toolchain: &str,
    _working_directory: Option<&str>,
    env: BTreeMap<String, String>,
) -> Result<Step, RenderError> {
    super::mbx_preflight::validate_rust_toolchain(rust_toolchain)?;
    let payload = shell_literal(COMPLETE_STATE_TRANSPORT_EXPR)?;
    let root = shell_literal(COMPLETE_STATE_ROOT_EXPR)?;
    let (parent, base) = split_shell(&root)?;
    let (payload_parent, _payload_base) = split_shell(&payload)?;
    let ceiling = CacheFamily::MbxBundle.spec().ceiling_bytes;
    let script = [
        "set -eu".to_owned(),
        RUNNER_TEMP_GUARD.to_owned(),
        format!("payload=\"{payload}\""),
        format!("root=\"{root}\""),
        format!("parent=\"{parent}\""),
        format!("base=\"{base}\""),
        format!("mark=\"{MARK_FILE}\""),
        "out=${GITHUB_OUTPUT-}".to_owned(),
        "skip() { if [ -n \"$out\" ]; then printf '%s\\n' 'exported=false' >> \"$out\"; fi; printf 'velnor: state snapshot not published (%s)\\n' \"$1\"; exit 0; }".to_owned(),
        "if [ ! -f \"$mark\" ] || [ -L \"$mark\" ]; then skip producer_not_succeeded; fi".to_owned(),
        format!("[ -n \"${{{MBX_EXPORT_GROUP_ENV}:-}}\" ] || skip export_group_missing"),
        "if [ ! -d \"$root\" ] || [ -L \"$root\" ]; then skip state_not_built; fi".to_owned(),
        "command -v tar >/dev/null 2>&1 || { printf '%s\\n' '::warning::cache_unavailable: tar is missing; save refused'; skip tar_missing; }".to_owned(),
        "command -v wc >/dev/null 2>&1 || { printf '%s\\n' '::warning::cache_unavailable: wc is missing; save refused'; skip wc_missing; }".to_owned(),
        format!("mkdir -p \"{payload_parent}\""),
        "rm -f \"$payload\"".to_owned(),
        "if tar -czf \"$payload\" -C \"$parent\" \"$base\"; then :; else rm -f \"$payload\"; printf '%s\\n' '::warning::cache_unavailable: state snapshot pack failed'; skip pack_failed; fi".to_owned(),
        "if [ ! -f \"$payload\" ] || [ -L \"$payload\" ]; then printf '%s\\n' '::warning::cache_unavailable: state payload missing after pack'; skip pack_missing; fi".to_owned(),
        format!("wc -c < \"$payload\" > \"{SIZE_FILE}\""),
        format!("size=; rest=; read -r size rest < \"{SIZE_FILE}\" || [ -n \"$size\" ]"),
        "case \"$size\" in ''|*[!0-9]*) rm -f \"$payload\"; skip size_unreadable ;; esac".to_owned(),
        format!("if [ \"$size\" -gt {ceiling} ]; then rm -f \"$payload\"; printf '%s\\n' '::warning::over_ceiling: state snapshot exceeds the family ceiling; save refused'; skip over_ceiling; fi"),
        "if [ \"$size\" -eq 0 ]; then rm -f \"$payload\"; skip empty_snapshot; fi".to_owned(),
        "if [ -n \"$out\" ]; then printf '%s\\n' 'exported=true' >> \"$out\"; fi".to_owned(),
        "printf 'velnor: state snapshot published (%s bytes)\\n' \"$size\"".to_owned(),
    ]
    .join("; ");
    let mut step = shell_step(
        MBX_BUNDLE_EXPORT_NAME,
        vec!["sh".to_owned(), "-c".to_owned(), script],
        env,
    )?;
    step.id = Some(StepId::MbxBundleExport);
    step.condition = Some(MBX_BUNDLE_EXPORT_CONDITION.to_owned());
    step.role = None;
    Ok(step)
}

fn save_step() -> Result<Step, RenderError> {
    let mut step = super::cache_action_step(
        false,
        crate::cache_steps::TOOLS_SAVE_USES,
        "mbx-bundle",
        MBX_BUNDLE_KEY_EXPR,
        &[],
        &[COMPLETE_STATE_TRANSPORT_EXPR.to_owned()],
    )?;
    MBX_BUNDLE_SAVE_NAME.clone_into(&mut step.name);
    step.condition = Some(MBX_BUNDLE_SAVE_CONDITION.to_owned());
    step.role = Some(StepRole::MbxBundleSave);
    Ok(step)
}

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
