//! Cache and lane step templates over validated action refs.
//!
//! Covers MBX objects-mode restore, `actions/cache` restore/save, and
//! per-lane target directories; pins arrive validated.

use std::collections::BTreeMap;

use velnor_actions_contract::{Job, Step, StepKind};

use crate::{
    RenderError,
    steps::{action_step, validate_uses},
};

/// Pinned mr-boxington action name (objects mode).
pub const MBX_ACTION_NAME: &str = "jdx/mr-boxington-action";
/// Cache restore/save action names.
pub const CACHE_RESTORE_NAME: &str = "actions/cache/restore";
/// Cache save action name.
pub const CACHE_SAVE_NAME: &str = "actions/cache/save";
/// Task-artifacts dir archived for task-result reuse.
pub const TASK_ARTIFACTS_DIR: &str = "$MISE_TASK_CACHE_DIR/task-artifacts/v2";
/// Target-directory prefix isolating one lane.
pub const TARGET_DIR_PREFIX: &str = "$RUNNER_TEMP/velnor/target/";
/// Pinned `actions/cache/restore` ref (v6.1.0, qualified 2026-09-28).
pub const TOOLS_RESTORE_USES: &str =
    "actions/cache/restore@55cc8345863c7cc4c66a329aec7e433d2d1c52a9";
/// Pinned `actions/cache/save` ref (v6.1.0, qualified 2026-09-28).
pub const TOOLS_SAVE_USES: &str = "actions/cache/save@55cc8345863c7cc4c66a329aec7e433d2d1c52a9";
/// Display name of the tools restore step.
pub const TOOLS_RESTORE_NAME: &str = "Restore Mise tools";
/// Display name of the tools save step.
pub const TOOLS_SAVE_NAME: &str = "Save Mise tools";
/// Sole tools-cache path: the default mise data dir.
pub const TOOLS_CACHE_PATH: &str = "~/.local/share/mise";
/// Tools-cache key namespace.
pub const TOOLS_KEY_PREFIX: &str = "mise-tools-v1";
/// Tool files hashed into the tools key (literal names, never globs).
const TOOLS_KEY_FILES: &str = "'mise.toml','.mise.toml','mise.lock','.mise.lock','.tool-versions'";

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
fn is_mbx_action(step: &Step) -> bool {
    matches!(&step.kind, velnor_actions_contract::StepKind::Action { uses, .. } if uses.starts_with(&format!("{MBX_ACTION_NAME}@")))
}

/// True when shell argv invokes the `mbx` program or tool spec.
fn uses_mbx_tool(step: &Step) -> bool {
    matches!(&step.kind, velnor_actions_contract::StepKind::Shell { run, .. } if run.iter().any(|arg| arg == "mbx" || arg.contains("mr-boxington")))
}

/// Objects-mode MBX step; cargo profiles must never emit or install MBX.
///
/// The action installs exactly `mbx_version` (the catalog pin): without
/// the `version` input the action resolves `latest`, and an action-SHA
/// pin never proves the installed executable (P07 effective-version
/// defect; the action documents that setting `version` always installs
/// that release: `https://github.com/jdx/mr-boxington-action`).
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
    ]);
    action_step("Restore MBX objects", uses, with)
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
/// # Errors
pub fn cache_action_step(
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
    if !matches!(layer, "sources" | "task" | "tools") {
        return Err(RenderError::BadCommand("mbx_needs_objects_mode".to_owned()));
    }
    if key.trim().is_empty() || key.contains(' ') || key.contains('\n') {
        return Err(RenderError::BadCommand("bad_cache_key".to_owned()));
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
    if restore {
        with.insert("restore-keys".to_owned(), restore_keys.join("\n"));
    }
    let name = if restore {
        "Restore cache"
    } else {
        "Save cache"
    };
    action_step(name, uses, with)
}

fn validate_cache_path(layer: &str, path: &str) -> Result<(), RenderError> {
    let second = path.split('/').nth(1);
    let legacy_ok = path.starts_with("$CARGO_HOME/") && matches!(second, Some("registry" | "git"));
    if layer == "sources" && (legacy_ok || sources_subset_ok(path))
        || layer == "task" && path == TASK_ARTIFACTS_DIR
        || layer == "tools" && path == TOOLS_CACHE_PATH
    {
        Ok(())
    } else {
        Err(RenderError::BadCommand(format!("bad_cache_path:{path}")))
    }
}

/// P08 sources subset under the owned Cargo home expression.
fn sources_subset_ok(path: &str) -> bool {
    const HOME: &str = "${{ runner.temp }}/velnor/cargo";
    if path.contains("..") || path.contains("credentials") {
        return false;
    }
    let Some(suffix) = path.strip_prefix(&format!("{HOME}/")) else {
        return false;
    };
    if suffix.starts_with("registry/src") {
        return false;
    }
    matches!(
        suffix,
        ".crates.toml" | ".crates2.json" | "bin" | "registry/index" | "registry/cache" | "git/db"
    ) || suffix.starts_with("registry/index/")
        || suffix.starts_with("registry/cache/")
        || suffix.starts_with("git/db/")
        || suffix.starts_with("bin/")
}

/// Tools-cache key: target, mise, generator, job, plus tool-file hash.
///
/// Static segments invalidate exactly when pins change; the trailing
/// `hashFiles` over literal tool-file names (no workspace walk, no
/// ELOOP) churns the key when tool files change. No spaces: the cache
/// action rejects them.
/// # Errors
pub fn tools_cache_key(
    target: &str,
    mise_version: &str,
    generator_version: &str,
    job_id: &str,
) -> Result<String, RenderError> {
    if !velnor_actions_contract::is_supported_target(target) {
        return Err(RenderError::BadCommand(format!(
            "bad_cache_target:{target}"
        )));
    }
    for (label, value) in [
        ("mise", mise_version),
        ("generator", generator_version),
        ("job", job_id),
    ] {
        if !is_key_segment(value) {
            return Err(RenderError::BadCommand(format!(
                "bad_cache_key_{label}:{value}"
            )));
        }
    }
    Ok(format!(
        "{TOOLS_KEY_PREFIX}-{target}-{mise_version}-{generator_version}-{job_id}-${{{{hashFiles({TOOLS_KEY_FILES})}}}}"
    ))
}

/// Prefix restore key: same pins and job, any tool-file hash.
fn tools_restore_keys(key: &str) -> Vec<String> {
    vec![tools_key_prefix_of(key)]
}

/// Key minus its trailing hash expression, kept as a prefix.
///
/// Splits on the expression marker (never on `-`: file names inside
/// the hash carry dashes); marker-less keys degrade to a full prefix.
fn tools_key_prefix_of(key: &str) -> String {
    key.split_once("${{")
        .map_or_else(|| format!("{key}-"), |(head, _)| head.to_owned())
}

/// Tools restore step over the pinned restore action.
/// # Errors
pub fn tools_restore_step(key: &str) -> Result<Step, RenderError> {
    let restore_keys = tools_restore_keys(key);
    let step = cache_action_step(
        true,
        TOOLS_RESTORE_USES,
        "tools",
        key,
        &restore_keys,
        &[TOOLS_CACHE_PATH.to_owned()],
    )?;
    rename_step(step, TOOLS_RESTORE_NAME)
}

/// Tools save step over the pinned save action.
/// # Errors
pub fn tools_save_step(key: &str) -> Result<Step, RenderError> {
    let step = cache_action_step(
        false,
        TOOLS_SAVE_USES,
        "tools",
        key,
        &[],
        &[TOOLS_CACHE_PATH.to_owned()],
    )?;
    rename_step(step, TOOLS_SAVE_NAME)
}

/// Rename a built step; names are fixed by the caller contract.
fn rename_step(mut step: Step, name: &str) -> Result<Step, RenderError> {
    crate::steps::scan_for_private_subcommands(name)?;
    name.clone_into(&mut step.name);
    Ok(step)
}

// P08: the manual `ensure_tools_cache` wrapper is removed. Tools use the
// Mise action's built-in cache (`cache_p08::ensure_setup_p08`); the
// `tools_*` constructors above remain for unit-test compatibility only
// and are never emitted into strict workflows.

/// Key segments: nonempty alphanumerics plus `.-_`, never `latest`.
fn is_key_segment(value: &str) -> bool {
    !value.is_empty()
        && !value.contains("latest")
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b'_'))
}

/// Isolated target directory for one lane.
#[must_use]
pub fn target_dir_for_lane(lane_id: &str) -> String {
    format!("{TARGET_DIR_PREFIX}{lane_id}")
}

/// `CARGO_TARGET_DIR` env pair isolating one lane (CACHE-1.20).
#[must_use]
pub fn lane_cargo_target_env(lane_id: &str) -> (String, String) {
    ("CARGO_TARGET_DIR".to_owned(), target_dir_for_lane(lane_id))
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
