//! Cache and lane step templates over validated action refs.
//!
//! Covers MBX objects-mode restore, `actions/cache` restore/save, and
//! per-lane target directories; pins arrive validated.

use std::collections::BTreeMap;

use velnor_actions_contract::{CompiledSourceHelper, Job, Step, StepKind};

use crate::{
    RenderError,
    steps::{action_step, action_step_with_env, validate_uses},
};

#[path = "cache_steps_mbx.rs"]
mod mbx;
#[path = "cache_steps_tools.rs"]
mod tools;

pub use tools::{
    TOOLS_CACHE_PATH, TOOLS_RESTORE_NAME, TOOLS_RESTORE_USES, TOOLS_SAVE_NAME, TOOLS_SAVE_USES,
    tool_payload_paths, tools_restore_step, tools_save_step,
};
pub(crate) use tools::{TOOLS_RESTORE_ID, is_tool_payload_path};

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

pub use velnor_actions_contract::CompilerDriver;

/// MBX objects step only for MBX-selected profiles; Cargo yields none.
///
/// Workflow contract §3 emits `jdx/mr-boxington-action` only when the
/// Rust detector selects MBX; Cargo legs carry neither the action nor
/// the tool (task-execution contract prelude). `mbx_version` is the
/// catalog pin the action installs (P07 effective-version closure).
/// # Errors
pub fn mbx_step_for_driver(
    uses: &str,
    driver: CompilerDriver,
    mbx_version: &str,
) -> Result<Option<Step>, RenderError> {
    match driver {
        CompilerDriver::Cargo => Ok(None),
        CompilerDriver::Mbx => mbx_objects_step(uses, false, mbx_version).map(Some),
    }
}

/// Gate MBX action/tool presence against per-job driver selections.
///
/// Jobs without a declared driver are skipped (plan/final/lint carry
/// none); declared Cargo jobs must be MBX-free. MBX jobs execute the
/// selected compiler; optional objects transport is admitted independently
/// after tool normalization, including an explicit cold outcome.
/// # Errors
pub fn check_mbx_gating(
    jobs: &BTreeMap<String, Job>,
    drivers: &BTreeMap<String, CompilerDriver>,
    records: &[CompiledSourceHelper],
) -> Result<(), RenderError> {
    for (id, driver) in drivers {
        let Some(job) = jobs.get(id.as_str()) else {
            return Err(RenderError::InvalidWorkflow(format!(
                "mbx_gating_unknown_job:{id}"
            )));
        };
        check_job_mbx(id, job, *driver, records)?;
    }
    Ok(())
}

/// Enforce one job's MBX presence against its declared driver.
fn check_job_mbx(
    id: &str,
    job: &Job,
    driver: CompilerDriver,
    records: &[CompiledSourceHelper],
) -> Result<(), RenderError> {
    let actions = job.steps.iter().filter(|step| is_mbx_action(step)).count();
    let tools = job
        .steps
        .iter()
        .any(|step| mbx::uses_mbx_tool(step) && !is_mbx_action(step));
    let execution = compiler_executions(job, records)?;
    match driver {
        CompilerDriver::Cargo => {
            if actions > 0 {
                return Err(RenderError::InvalidWorkflow(format!(
                    "mbx_action_without_selection:{id}"
                )));
            }
            if tools || execution.contains(&velnor_actions_contract::CompilerDriver::Mbx) {
                return Err(RenderError::InvalidWorkflow(format!(
                    "mbx_tool_without_selection:{id}"
                )));
            }
        }
        CompilerDriver::Mbx => {
            if actions > 1 {
                return Err(RenderError::InvalidWorkflow(format!("mbx_duplicated:{id}")));
            }
            if !execution.contains(&velnor_actions_contract::CompilerDriver::Mbx) {
                return Err(RenderError::InvalidWorkflow(format!(
                    "mbx_missing_for_selection:{id}"
                )));
            }
            if execution.contains(&velnor_actions_contract::CompilerDriver::Cargo) {
                return Err(RenderError::InvalidWorkflow(format!(
                    "mbx_compiler_selection_mismatch:{id}"
                )));
            }
        }
    }
    Ok(())
}

/// Compiler authority is an opaque source record bound to this exact invocation.
fn compiler_executions(
    job: &Job,
    records: &[CompiledSourceHelper],
) -> Result<Vec<velnor_actions_contract::CompilerDriver>, RenderError> {
    let mut drivers = Vec::new();
    for step in &job.steps {
        let StepKind::SourceBoundHelper { invocation, env } = &step.kind else {
            continue;
        };
        let matches: Vec<_> = records
            .iter()
            .filter(|record| {
                record.invocation() == invocation
                    && record.environment() == env
                    && record.compiler_driver().is_some()
            })
            .collect();
        if matches.len() > 1 {
            return Err(RenderError::InvalidWorkflow(
                "mbx_ambiguous_compiler_execution".to_owned(),
            ));
        }
        for record in matches {
            record.validate_binding().map_err(RenderError::Contract)?;
            if let Some(driver) = record.compiler_driver() {
                drivers.push(driver);
            }
        }
    }
    Ok(drivers)
}

/// True for `jdx/mr-boxington-action` steps.
fn is_mbx_action(step: &Step) -> bool {
    matches!(&step.kind, velnor_actions_contract::StepKind::Action { uses, .. } if uses.starts_with(&format!("{MBX_ACTION_NAME}@")))
}

/// Env key the cache backend reads for its restore/save mode.
pub const MBX_CACHE_MODE_ENV: &str = "ACTIONS_CACHE_MODE";
/// Display name of the MBX objects restore step.
pub const MBX_RESTORE_NAME: &str = "Restore MBX objects";

/// Objects-mode MBX step; cargo profiles must never emit or install MBX.
///
/// The action installs exactly `mbx_version` (the catalog pin): without
/// the `version` input the action resolves `latest`, and an action-SHA
/// pin never proves the installed executable (P07 effective-version
/// defect; the action documents that setting `version` always installs
/// that release: `https://github.com/jdx/mr-boxington-action`).
/// The bundled cache client saves from the success-only post step.
/// Trusted writes depend on the pinned action's default-branch push
/// check and GitHub's server cache scope. [`MBX_CACHE_MODE_ENV`] is a
/// defensive client hint; it is not a server-enforced trust boundary.
/// Every action upgrade must revalidate its post-save event/ref policy.
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
    let env = BTreeMap::from([(
        MBX_CACHE_MODE_ENV.to_owned(),
        velnor_actions_contract::workflow::ir::CACHE_MODE_PUSH_WRITE_EXPR.to_owned(),
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
    if !matches!(layer, "sources" | "task" | "tools" | "tofu-providers") {
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
        || layer == "tools" && is_tool_payload_path(path)
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

/// Check restore-before/save-after ordering over cache action steps.
///
/// Every payload `actions/cache/restore` step (plus MBX objects restore) must
/// precede every `actions/cache/save` step within one job. Exact `lookup-only`
/// receipts do not import payloads and may follow publication.
/// # Errors
pub fn check_cache_step_order(steps: &[Step]) -> Result<(), RenderError> {
    let mut last_restore: Option<usize> = None;
    let mut first_save: Option<usize> = None;
    for (index, step) in steps.iter().enumerate() {
        let StepKind::Action { uses, with, .. } = &step.kind else {
            continue;
        };
        if (uses.starts_with("actions/cache/restore@")
            && with.get("lookup-only").map(String::as_str) != Some("true"))
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
