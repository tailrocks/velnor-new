//! Cache and lane step templates over validated action refs.
//!
//! Covers MBX objects-mode restore, `actions/cache` restore/save, and
//! per-lane target directories; pins arrive validated.

use std::collections::BTreeMap;

use velnor_actions_contract::{Step, StepKind};

#[path = "cache_steps_mbx_command.rs"]
mod mbx_command;
#[path = "cache_steps_mbx_gate.rs"]
mod mbx_gate;
#[path = "cache_steps_mbx_preflight.rs"]
mod mbx_preflight;
pub(crate) use mbx_gate::append_workspace_cleanups;
pub use mbx_gate::check_mbx_gating;
pub(crate) use mbx_gate::is_mbx_action;
pub use mbx_preflight::{
    MBX_CACHE_MODE_ENV, MBX_PREFLIGHT_NAME, MBX_RESTORE_NAME, MBX_VERSION_CHECK_NAME,
    mbx_steps_for_driver,
};
pub(crate) use mbx_preflight::{
    MBX_GC_AUTO_ENV, MBX_GC_AUTO_VALUE, MBX_SHARE_OUT_DIR_ENV, MBX_SHARE_OUT_DIR_VALUE,
    mbx_workspace_clean_step_for_action,
};

use crate::{
    RenderError,
    steps::{action_step, validate_uses},
};

#[path = "cache_steps_tools.rs"]
mod tools;

pub use tools::{
    TOOLS_CACHE_PATH, TOOLS_CACHE_PATHS, TOOLS_RESTORE_ACTION_USES, TOOLS_RESTORE_NAME,
    TOOLS_RESTORE_USES, TOOLS_SAVE_NAME, TOOLS_SAVE_USES,
};
pub(crate) use tools::{TOOLS_SEED_ADMITTED_EXPRESSION, TOOLS_SEED_ADMITTED_INPUT};

#[cfg(test)]
pub(crate) use tools::assert_rendered_admission_parses;
pub(crate) use tools::restore_action_file as tools_restore_action_file;

/// Build one V2 tools-cache action over the fixed path set.
/// # Errors
pub(crate) fn tools_cache_step(
    restore: bool,
    key: &str,
    condition: Option<String>,
) -> Result<Step, RenderError> {
    tools::cache_step(restore, key, condition)
}

pub(crate) fn validate_tools_restore_call(step: &Step) -> Result<&str, RenderError> {
    tools::validate_restore_call(step)
}

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
    if layer == "sources" && sources_subset_ok(path)
        || layer == "task" && path == TASK_ARTIFACTS_DIR
        || layer == "tools" && tools::tools_cache_path_ok(path)
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
        if uses == TOOLS_RESTORE_USES
            || uses.starts_with("actions/cache/restore@")
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
