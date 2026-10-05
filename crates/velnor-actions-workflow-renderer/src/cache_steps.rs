//! Cache and lane step templates over validated action refs.
//!
//! Covers MBX objects-mode restore, `actions/cache` restore/save, and
//! per-lane target directories; pins arrive validated.

use std::collections::BTreeMap;

use velnor_actions_contract::cachekey::mbx_cache_generation;
use velnor_actions_contract::workflow::ir::CACHE_SAVE_CONDITION;
use velnor_actions_contract::{Job, Step, StepKind};

use crate::{
    RenderError,
    steps::{action_step, action_step_with_env, validate_uses},
};

#[path = "cache_steps_tools.rs"]
mod tools;

pub use tools::{
    TOOLS_CACHE_ELIGIBLE_ENV, TOOLS_CACHE_PATHS, TOOLS_CACHE_RESTORE_CONDITION,
    TOOLS_CACHE_SAVE_CONDITION, TOOLS_IMAGE_IDENTITY_NAME, TOOLS_IMAGE_OS_ENV,
    TOOLS_IMAGE_VERSION_ENV, TOOLS_KEY_PREFIX, TOOLS_MISE_BOOTSTRAP_BINARY,
    TOOLS_MISE_BOOTSTRAP_DATA_DIR, TOOLS_MISE_DATA_DIR, TOOLS_RESTORE_NAME, TOOLS_RESTORE_USES,
    TOOLS_SAVE_NAME, TOOLS_SAVE_USES, is_tools_cache_key, is_tools_cache_path,
    tools_cache_image_identity_step, tools_cache_path_input, tools_restore_step, tools_save_step,
};

/// Pinned mr-boxington action name (objects mode).
pub const MBX_ACTION_NAME: &str = "jdx/mr-boxington-action";
/// Cache restore/save action names.
pub const CACHE_RESTORE_NAME: &str = "actions/cache/restore";
/// Cache save action name.
pub const CACHE_SAVE_NAME: &str = "actions/cache/save";
/// Task-artifacts dir archived for task-result reuse.
pub const TASK_ARTIFACTS_DIR: &str = "$MISE_TASK_CACHE_DIR/task-artifacts/v2";

/// Never-archive markers: state, plans, and credential-bearing names
/// must never enter a cache archive (T22).
///
/// Exact mirror of the Mise list (`cache_sources`); the orchestrator
/// pins both equal by test, like the credential denylists.
pub const NEVER_ARCHIVE_MARKERS: [&str; 3] = ["credentials", ".tfstate", ".tfplan"];

/// True when `path` names state, plans, or credentials.
#[must_use]
pub fn is_never_archive_path(path: &str) -> bool {
    NEVER_ARCHIVE_MARKERS
        .iter()
        .any(|marker| path.contains(marker))
}

/// Detected Rust compile driver (task-execution contract profile).
///
/// The orchestrator maps the workspace's detected profile to this typed
/// selector; the renderer never inspects evidence itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompileDriver {
    /// Cargo profile: MBX action and installation are absent.
    Cargo,
    /// MBX profile: the objects-mode action restores compiler objects.
    Mbx,
}

/// MBX objects step only for MBX-selected profiles; Cargo yields none.
///
/// Workflow contract §3 emits `jdx/mr-boxington-action` only when the
/// Rust detector selects MBX; Cargo legs carry neither the action nor
/// the tool (task-execution contract prelude). `mbx_version` is the
/// catalog pin the action installs (P07 effective-version closure).
/// # Errors
pub fn mbx_step_for_driver(
    uses: &str,
    driver: CompileDriver,
    mbx_version: &str,
) -> Result<Option<Step>, RenderError> {
    match driver {
        CompileDriver::Cargo => Ok(None),
        CompileDriver::Mbx => mbx_objects_step(uses, false, mbx_version).map(Some),
    }
}

/// Gate MBX action/tool presence against per-job driver selections.
///
/// Jobs without a declared driver are skipped (plan/final/lint carry
/// none); declared Cargo jobs must be MBX-free while MBX jobs carry
/// exactly one objects-mode step.
/// # Errors
pub fn check_mbx_gating(
    jobs: &BTreeMap<String, Job>,
    drivers: &BTreeMap<String, CompileDriver>,
) -> Result<(), RenderError> {
    for (id, driver) in drivers {
        let Some(job) = jobs.get(id.as_str()) else {
            return Err(RenderError::InvalidWorkflow(format!(
                "mbx_gating_unknown_job:{id}"
            )));
        };
        check_job_mbx(id, job, *driver)?;
    }
    Ok(())
}

/// Enforce one job's MBX presence against its declared driver.
fn check_job_mbx(id: &str, job: &Job, driver: CompileDriver) -> Result<(), RenderError> {
    let actions = job.steps.iter().filter(|step| is_mbx_action(step)).count();
    let tools = job
        .steps
        .iter()
        .any(|step| uses_mbx_tool(step) && !is_mbx_action(step));
    match driver {
        CompileDriver::Cargo => {
            if actions > 0 {
                return Err(RenderError::InvalidWorkflow(format!(
                    "mbx_action_without_selection:{id}"
                )));
            }
            if tools {
                return Err(RenderError::InvalidWorkflow(format!(
                    "mbx_tool_without_selection:{id}"
                )));
            }
        }
        CompileDriver::Mbx => {
            if actions == 0 {
                return Err(RenderError::InvalidWorkflow(format!(
                    "mbx_missing_for_selection:{id}"
                )));
            }
            if actions > 1 {
                return Err(RenderError::InvalidWorkflow(format!("mbx_duplicated:{id}")));
            }
        }
    }
    Ok(())
}

/// True for `jdx/mr-boxington-action` steps.
pub(crate) fn is_mbx_action(step: &Step) -> bool {
    matches!(&step.kind, velnor_actions_contract::StepKind::Action { uses, .. } if uses.starts_with(&format!("{MBX_ACTION_NAME}@")))
}

/// True when shell argv invokes the `mbx` program or tool spec.
fn uses_mbx_tool(step: &Step) -> bool {
    matches!(&step.kind, velnor_actions_contract::StepKind::Shell { run, .. } if run.iter().any(|arg| arg == "mbx" || arg.contains("mr-boxington")))
}

/// Env key the cache backend reads for its restore/save mode.
pub const MBX_CACHE_MODE_ENV: &str = "ACTIONS_CACHE_MODE";
/// Display name of the MBX objects restore step.
pub const MBX_RESTORE_NAME: &str = "Restore MBX objects";
/// MBX automatic collection must stay enabled so low-disk builds can recover.
pub(crate) const MBX_GC_AUTO_ENV: &str = "MBX_GC_AUTO";
/// MBX 1.21.1+ honors this value and protects active build consumers.
pub(crate) const MBX_GC_AUTO_VALUE: &str = "1";
/// Mode that skips the action post. `read` does not permit writes.
pub(crate) const MBX_ACTION_CACHE_MODE: &str = "read";

/// Objects-mode MBX step; cargo profiles must never emit or install MBX.
///
/// The action installs exactly `mbx_version` (the catalog pin): without
/// the `version` input the action resolves `latest`, and an action-SHA
/// pin never proves the installed executable (P07 effective-version
/// defect; the action documents that setting `version` always installs
/// that release: `https://github.com/jdx/mr-boxington-action`).
/// The step-level [`MBX_CACHE_MODE_ENV`] is the literal `read`. The
/// action's post exports inside the live store and then archives that
/// copy, and a default-branch push saves even when `save-on-*` is off.
/// `read` is the switch that skips that post. Velnor's later single-bundle
/// step uses the shared protected-default writer gate.
/// The cache generation follows the exact MBX release, so upgrading its
/// storage or collection behavior starts with an isolated cold namespace.
/// # Errors
pub fn mbx_objects_step(
    uses: &str,
    cargo_profile: bool,
    mbx_version: &str,
) -> Result<Step, RenderError> {
    if cargo_profile {
        return Err(RenderError::BadCommand("cargo_profile_no_mbx".to_owned()));
    }
    validate_uses(uses)?;
    if !uses.starts_with(&format!("{MBX_ACTION_NAME}@")) {
        return Err(RenderError::BadActionRef(format!("not_mbx_action:{uses}")));
    }
    if !is_exact_mbx_version(mbx_version) {
        return Err(RenderError::BadCommand(format!(
            "bad_mbx_version:{mbx_version}"
        )));
    }
    let with = BTreeMap::from([
        ("github-cache-mode".to_owned(), "objects".to_owned()),
        ("version".to_owned(), mbx_version.to_owned()),
        (
            "cache-generation".to_owned(),
            mbx_cache_generation(mbx_version),
        ),
    ]);
    let env = BTreeMap::from([(
        MBX_CACHE_MODE_ENV.to_owned(),
        MBX_ACTION_CACHE_MODE.to_owned(),
    )]);
    action_step_with_env(MBX_RESTORE_NAME, uses, with, env)
}

/// Exact MBX versions: three nonempty numeric dot parts, nothing else.
fn is_exact_mbx_version(version: &str) -> bool {
    let parts: Vec<&str> = version.split('.').collect();
    parts.len() == 3
        && parts
            .iter()
            .all(|part| !part.is_empty() && part.bytes().all(|b| b.is_ascii_digit()))
}

/// Cache restore/save step over `actions/cache`; MBX never archives here.
/// Every generic saver gets the protected-default success gate at this
/// constructor boundary. The tool-cache wrapper extends it with image
/// eligibility; no caller can accidentally emit an ungated save.
/// # Errors
pub fn cache_action_step(
    restore: bool,
    uses: &str,
    layer: &str,
    key: &str,
    restore_keys: &[String],
    paths: &[String],
) -> Result<Step, RenderError> {
    if layer == "tools" {
        return Err(RenderError::BadCommand(
            "tools_cache_requires_typed_builder".to_owned(),
        ));
    }
    cache_action_step_inner(restore, uses, layer, key, restore_keys, paths)
}

/// Construct one canonical tools action; only the typed restore/save
/// wrappers in the child module may call this path.
fn tools_cache_action_step(
    restore: bool,
    uses: &str,
    key: &str,
    paths: &[String],
) -> Result<Step, RenderError> {
    cache_action_step_inner(restore, uses, "tools", key, &[], paths)
}

/// Shared constructor after public layer policy has been checked.
fn cache_action_step_inner(
    restore: bool,
    uses: &str,
    layer: &str,
    key: &str,
    restore_keys: &[String],
    paths: &[String],
) -> Result<Step, RenderError> {
    validate_uses(uses)?;
    let want = if restore {
        CACHE_RESTORE_NAME
    } else {
        CACHE_SAVE_NAME
    };
    if !uses.starts_with(&format!("{want}@")) {
        return Err(RenderError::BadActionRef(format!("bad_cache_uses:{uses}")));
    }
    if !matches!(layer, "sources" | "task" | "tools" | "tofu-providers") {
        return Err(RenderError::BadCommand("mbx_needs_objects_mode".to_owned()));
    }
    if key.trim().is_empty()
        || key.contains('\n')
        || layer != "tools" && key.contains(' ')
        || layer == "tools" && !is_tools_cache_key(key)
    {
        return Err(RenderError::BadCommand("bad_cache_key".to_owned()));
    }
    if layer == "tools" && !restore_keys.is_empty() {
        return Err(RenderError::BadCommand(
            "tools_cache_restore_prefix_forbidden".to_owned(),
        ));
    }
    if paths.is_empty() {
        return Err(RenderError::BadCommand("empty_cache_paths".to_owned()));
    }
    for path in paths {
        validate_cache_path(layer, path)?;
    }
    let mut with = BTreeMap::from([
        ("key".to_owned(), key.to_owned()),
        ("path".to_owned(), paths.join("\n")),
    ]);
    if restore && !restore_keys.is_empty() {
        with.insert("restore-keys".to_owned(), restore_keys.join("\n"));
    }
    let name = if restore {
        "Restore cache"
    } else {
        "Save cache"
    };
    let mut step = action_step(name, uses, with)?;
    if !restore {
        step.condition =
            Some(velnor_actions_contract::workflow::ir::CACHE_SAVE_CONDITION.to_owned());
    }
    Ok(step)
}

fn validate_cache_path(layer: &str, path: &str) -> Result<(), RenderError> {
    if layer == "sources" && sources_subset_ok(path)
        || layer == "task" && path == TASK_ARTIFACTS_DIR
        || layer == "tools" && is_tools_cache_path(path)
        || layer == "tofu-providers" && crate::tofu_cache::tofu_providers_path_ok(path)
    {
        Ok(())
    } else {
        Err(RenderError::BadCommand(format!("bad_cache_path:{path}")))
    }
}

/// P08 sources subset under the owned Cargo home expression.
fn sources_subset_ok(path: &str) -> bool {
    const HOME: &str = "${{ runner.temp }}/velnor/cargo";
    if path.contains("..") || is_never_archive_path(path) {
        return false;
    }
    let Some(suffix) = path.strip_prefix(&format!("{HOME}/")) else {
        return false;
    };
    if suffix.starts_with("registry/src") {
        return false;
    }
    matches!(suffix, "registry/index" | "registry/cache" | "git/db")
        || suffix.starts_with("registry/index/")
        || suffix.starts_with("registry/cache/")
        || suffix.starts_with("git/db/")
}

/// Add the protected-default writer gate to every rendered cache-saving action.
///
/// A caller may add a stricter condition, but cannot replace this base gate.
pub(crate) fn action_cache_condition(uses: &str, condition: Option<&str>) -> Option<String> {
    let writes_cache =
        uses.starts_with("actions/cache/save@") || uses.starts_with("actions/cache@");
    if !writes_cache {
        return condition.map(str::to_owned);
    }
    Some(with_cache_save_gate(condition))
}

/// Add the protected-default gate to direct MBX bundle-export shell steps.
pub(crate) fn shell_cache_condition(argv: &[String], condition: Option<&str>) -> Option<String> {
    let exports_mbx = argv
        .windows(3)
        .any(|parts| parts[0] == "mbx" && parts[1] == "cache" && parts[2] == "export")
        || argv.iter().any(|arg| arg.contains("mbx cache export"));
    exports_mbx
        .then(|| with_cache_save_gate(condition))
        .or_else(|| condition.map(str::to_owned))
}

/// Preserve an already-leading shared gate without adding a duplicate.
///
/// Only a pure conjunction is already protected. Any disjunction is wrapped
/// again so a caller cannot bypass the shared gate with `||`.
fn with_cache_save_gate(condition: Option<&str>) -> String {
    let gate = CACHE_SAVE_CONDITION;
    match condition {
        Some(value) if value == gate => gate.to_owned(),
        Some(value)
            if value
                .strip_prefix(gate)
                .and_then(|rest| rest.strip_prefix(" && "))
                .is_some_and(|rest| !rest.contains("||")) =>
        {
            value.to_owned()
        }
        Some(value) => format!("{gate} && ({value})"),
        None => gate.to_owned(),
    }
}

/// Check restore-before/save-after ordering over cache action steps.
///
/// Every `actions/cache/restore` step (plus MBX objects restore) must
/// precede every `actions/cache/save` step within one job.
/// # Errors
pub fn check_cache_step_order(steps: &[Step]) -> Result<(), RenderError> {
    let mut last_restore: Option<usize> = None;
    let mut first_save: Option<usize> = None;
    for (index, step) in steps.iter().enumerate() {
        let StepKind::Action { uses, .. } = &step.kind else {
            continue;
        };
        if uses.starts_with("actions/cache/restore@")
            || uses.starts_with(&format!("{MBX_ACTION_NAME}@"))
        {
            last_restore = Some(index);
        } else if uses.starts_with("actions/cache/save@") {
            first_save.get_or_insert(index);
        }
    }
    if let (Some(restore), Some(save)) = (last_restore, first_save)
        && restore > save
    {
        return Err(RenderError::InvalidWorkflow(
            "cache_save_before_restore".to_owned(),
        ));
    }
    Ok(())
}
