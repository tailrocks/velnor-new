//! Cache and lane step templates over validated action refs.
//!
//! Covers MBX local setup, `actions/cache` restore/save, and
//! per-lane target directories; pins arrive validated.

use std::collections::BTreeMap;

use velnor_actions_contract::cachekey::{
    MBX_CACHE_GENERATION_PREFIX, cache_key, mbx_cache_generation, restore_prefix,
};
use velnor_actions_contract::{Job, RunsOn, Step, StepKind, digest_b3};

#[path = "cache_steps_mbx_command.rs"]
mod mbx_command;
#[path = "cache_steps_mbx_gate.rs"]
mod mbx_gate;
#[path = "cache_steps_mbx_preflight.rs"]
mod mbx_preflight;
pub(crate) use mbx_gate::append_workspace_cleanups;
pub use mbx_gate::check_mbx_gating;
pub(crate) use mbx_gate::is_mbx_action;
pub(crate) use mbx_preflight::{
    ACTION_MBX_BIN, MBX_GC_AUTO_ENV, MBX_GC_AUTO_VALUE, MBX_SHARE_OUT_DIR_ENV,
    MBX_SHARE_OUT_DIR_VALUE, RESOLVE_ACTION_MBX_BIN, mbx_version_check_step,
    mbx_workspace_clean_step_for_action,
};
pub use mbx_preflight::{
    MBX_CACHE_MODE_ENV, MBX_PREFLIGHT_NAME, MBX_RESTORE_NAME, MBX_VERSION_CHECK_NAME,
    mbx_steps_for_driver,
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

/// Pinned mr-boxington action name (local setup backend).
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

/// Keep the pinned MBX action in local-only mode and add the explicit
/// runner-cache transport only on supported hosted jobs.
///
/// A private per-job MBX store is established by the preceding PATH
/// preflight. Hosted jobs restore/import one directory bundle before their
/// producers and export/save it afterward; scale-set jobs have no remote
/// transport until their store ownership is qualified.
pub(crate) fn apply_mbx_cache_policy(
    jobs: &mut BTreeMap<String, Job>,
    default_label: &str,
) -> Result<(), RenderError> {
    for (id, job) in jobs {
        let cache_target = hosted_cache_target(job, default_label);
        let action_indices: Vec<usize> = job
            .steps
            .iter()
            .enumerate()
            .filter_map(|(index, step)| is_mbx_action(step).then_some(index))
            .collect();
        if action_indices.len() > 1 {
            return Err(RenderError::InvalidWorkflow(format!(
                "multiple_mbx_actions:{id}"
            )));
        }
        for action_index in action_indices.into_iter().rev() {
            let identity = configure_mbx_action(job, action_index, id)?;
            if let Some(target) = cache_target {
                let lifecycle = mbx_bundle_steps(id, job, target, &identity)?;
                insert_before_producers(job, action_index, lifecycle.restore)?;
                job.steps.extend(lifecycle.save);
            }
        }
    }
    Ok(())
}

struct MbxBundleLifecycle {
    restore: [Step; 2],
    save: [Step; 2],
}

fn configure_mbx_action(
    job: &mut Job,
    action_index: usize,
    id: &str,
) -> Result<(String, String), RenderError> {
    let (version, toolchain) = mbx_action_identity(&job.steps[action_index], id)?;
    let preflight = action_index
        .checked_sub(1)
        .and_then(|index| job.steps.get(index))
        .ok_or_else(|| RenderError::InvalidWorkflow(format!("mbx_preflight_missing:{id}")))?;
    if !mbx_preflight::matches_mbx_steps(
        preflight,
        &job.steps[action_index],
        &version,
        &toolchain,
    ) {
        return Err(RenderError::InvalidWorkflow(format!(
            "mbx_preflight_mismatch:{id}"
        )));
    }
    let StepKind::Action { with, env, .. } = &mut job.steps[action_index].kind else {
        return Err(RenderError::InvalidWorkflow(format!("mbx_action_invalid:{id}")));
    };
    validate_mbx_action_policy(with, env, id)?;
    with.insert("backend".to_owned(), "local".to_owned());
    Ok((version, toolchain))
}

fn validate_mbx_action_policy(
    with: &BTreeMap<String, String>,
    env: &BTreeMap<String, String>,
    id: &str,
) -> Result<(), RenderError> {
    if env.get(MBX_CACHE_MODE_ENV).map(String::as_str) != Some("read") {
        return Err(RenderError::InvalidWorkflow(format!(
            "mbx_cache_mode_not_read_only:{id}"
        )));
    }
    if ["MBX_CACHE_DIR", "MBX_CACHE_EXPORT_GROUP", "MBX_GC_AUTO"]
        .iter()
        .any(|key| env.contains_key(*key))
    {
        return Err(RenderError::InvalidWorkflow(format!(
            "mbx_store_policy_override:{id}"
        )));
    }
    if with.keys().any(|key| {
        key.starts_with("save-on-")
            || matches!(
                key.as_str(),
                "backend"
                    | "cache-key"
                    | "restore-keys"
                    | "isolate-objects-cache"
                    | "cache-key-suffix"
            )
    }) {
        return Err(RenderError::InvalidWorkflow(format!(
            "mbx_cache_policy_override:{id}"
        )));
    }
    Ok(())
}

fn mbx_bundle_steps(
    id: &str,
    job: &Job,
    target: &str,
    identity: &(String, String),
) -> Result<MbxBundleLifecycle, RenderError> {
    let (version, toolchain) = identity;
    let compatibility = mbx_compatibility_digest(id, target, toolchain, version);
    let snapshot = mbx_snapshot_digest(job);
    let trusted_key = cache_key("mbx", "trusted", &compatibility, &snapshot)
        .map_err(RenderError::Contract)?;
    let pr_key = cache_key("mbx", "pr", &compatibility, &snapshot)
        .map_err(RenderError::Contract)?;
    let trusted_prefix = restore_prefix("mbx", "trusted", &compatibility)
        .map_err(RenderError::Contract)?;
    let pr_prefix = restore_prefix("mbx", "pr", &compatibility)
        .map_err(RenderError::Contract)?;
    let trust_selector = "github.event_name == 'push' && github.ref == 'refs/heads/main' && github.ref_protected";
    let key = format!(
        "${{{{ {trust_selector} && '{trusted_key}' || '{pr_key}' }}}}-${{{{ github.sha }}}}"
    );
    let prefix = format!(
        "${{{{ {trust_selector} && '{trusted_prefix}' || '{pr_prefix}' }}}}"
    );
    let save_condition = format!(
        "success() && {trust_selector}"
    );
    let steps = crate::stock_mbx_bundle::lifecycle_steps(
        TOOLS_RESTORE_USES,
        TOOLS_SAVE_USES,
        &key,
        &prefix,
        &save_condition,
    )?;
    Ok(MbxBundleLifecycle {
        restore: steps.restore,
        save: steps.save,
    })
}

fn mbx_compatibility_digest(id: &str, target: &str, toolchain: &str, version: &str) -> String {
    let mut bytes = Vec::new();
    for value in [
        "velnor-mbx-directory-v1",
        id,
        target,
        toolchain,
        version,
    ] {
        append_identity_part(&mut bytes, value.as_bytes());
    }
    digest_b3(&bytes)
}

fn mbx_snapshot_digest(job: &Job) -> String {
    let mut bytes = Vec::new();
    for step in &job.steps {
        append_identity_part(&mut bytes, step.name.as_bytes());
        match &step.kind {
            StepKind::Shell { run, env } => {
                append_identity_part(&mut bytes, b"shell");
                for arg in run {
                    append_identity_part(&mut bytes, arg.as_bytes());
                }
                for (key, value) in env {
                    if matches!(key.as_str(), "MBX_CACHE_DIR" | "MBX_CACHE_EXPORT_GROUP") {
                        continue;
                    }
                    append_identity_part(&mut bytes, key.as_bytes());
                    append_identity_part(&mut bytes, value.as_bytes());
                }
            }
            StepKind::Action { uses, with, env } => {
                append_identity_part(&mut bytes, b"action");
                append_identity_part(&mut bytes, uses.as_bytes());
                for (key, value) in with {
                    append_identity_part(&mut bytes, key.as_bytes());
                    append_identity_part(&mut bytes, value.as_bytes());
                }
                for (key, value) in env {
                    append_identity_part(&mut bytes, key.as_bytes());
                    append_identity_part(&mut bytes, value.as_bytes());
                }
            }
            StepKind::Internal { operation, env } => {
                append_identity_part(&mut bytes, b"internal");
                append_identity_part(&mut bytes, operation.as_bytes());
                for (key, value) in env {
                    append_identity_part(&mut bytes, key.as_bytes());
                    append_identity_part(&mut bytes, value.as_bytes());
                }
            }
            StepKind::TaskExecution { argv, env, task_id, .. } => {
                append_identity_part(&mut bytes, b"task");
                append_identity_part(&mut bytes, task_id.as_bytes());
                for arg in argv {
                    append_identity_part(&mut bytes, arg.as_bytes());
                }
                for (key, value) in env {
                    append_identity_part(&mut bytes, key.as_bytes());
                    append_identity_part(&mut bytes, value.as_bytes());
                }
            }
        }
    }
    digest_b3(&bytes)
}

fn append_identity_part(buffer: &mut Vec<u8>, part: &[u8]) {
    buffer.extend_from_slice(&(part.len() as u64).to_be_bytes());
    buffer.extend_from_slice(part);
}

fn insert_before_producers(
    job: &mut Job,
    action_index: usize,
    restore: [Step; 2],
) -> Result<(), RenderError> {
    let insert_at = action_index
        .checked_add(1)
        .ok_or_else(|| RenderError::InvalidWorkflow("mbx_step_index_overflow".to_owned()))?;
    job.steps.splice(insert_at..insert_at, restore);
    Ok(())
}

include!("cache_steps.part2.rs");
