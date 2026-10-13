//! Cache and lane step templates over validated action refs.
//!
//! Covers MBX objects-mode restore, `actions/cache` restore/save, and
//! per-lane target directories; pins arrive validated.

use std::collections::BTreeMap;

use velnor_actions_contract::cachekey::{
    MBX_CACHE_GENERATION_PREFIX, cache_key as typed_cache_key,
    mbx_cache_generation, restore_prefix as typed_restore_prefix,
};
use velnor_actions_contract::{Job, RunsOn, Step, StepKind, digest_b3};

use crate::{
    RenderError,
    steps::{action_step, validate_uses},
};

#[path = "cache_steps_tools.rs"]
mod tools;
#[path = "cache_steps_mbx_preflight.rs"]
mod mbx_preflight;

pub use tools::{
    TOOLS_CACHE_PATH, TOOLS_KEY_PREFIX, TOOLS_RESTORE_NAME, TOOLS_RESTORE_USES, TOOLS_SAVE_NAME,
    TOOLS_SAVE_USES, tools_cache_key, tools_restore_step, tools_save_step,
};
pub use mbx_preflight::{
    MBX_CACHE_MODE_ENV, MBX_PREFLIGHT_NAME, MBX_RESTORE_NAME, mbx_steps_for_driver,
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
    let trusted_key = typed_cache_key("mbx", "trusted", &compatibility, &snapshot)
        .map_err(RenderError::Contract)?;
    let pr_key = typed_cache_key("mbx", "pr", &compatibility, &snapshot)
        .map_err(RenderError::Contract)?;
    let trusted_prefix = typed_restore_prefix("mbx", "trusted", &compatibility)
        .map_err(RenderError::Contract)?;
    let pr_prefix = typed_restore_prefix("mbx", "pr", &compatibility)
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
            StepKind::Internal { operation } => {
                append_identity_part(&mut bytes, b"internal");
                append_identity_part(&mut bytes, operation.as_bytes());
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

fn mbx_action_identity(step: &Step, id: &str) -> Result<(String, String), RenderError> {
    let StepKind::Action { with, .. } = &step.kind else {
        return Err(RenderError::InvalidWorkflow(format!("mbx_action_invalid:{id}")));
    };
    if with.get("github-cache-mode").map(String::as_str) != Some("objects")
        || with.contains_key("version")
    {
        return Err(RenderError::InvalidWorkflow(format!("mbx_action_inputs_invalid:{id}")));
    }
    let generation = with.get("cache-generation").ok_or_else(|| {
        RenderError::InvalidWorkflow(format!("mbx_cache_generation_missing:{id}"))
    })?;
    let version = generation
        .strip_prefix(MBX_CACHE_GENERATION_PREFIX)
        .filter(|version| is_exact_mbx_version(version))
        .ok_or_else(|| RenderError::InvalidWorkflow(format!("mbx_cache_generation_invalid:{id}")))?
        .to_owned();
    if *generation != mbx_cache_generation(&version) {
        return Err(RenderError::InvalidWorkflow(format!(
            "mbx_cache_generation_override:{id}"
        )));
    }
    let toolchain = with.get("toolchain").cloned().ok_or_else(|| {
        RenderError::InvalidWorkflow(format!("mbx_toolchain_missing:{id}"))
    })?;
    Ok((version, toolchain))
}

/// Whether a job maps to a supported hosted target rather than a selector.
pub(crate) fn has_hosted_cache_target(job: &Job, default_label: &str) -> bool {
    hosted_cache_target(job, default_label).is_some()
}

fn hosted_cache_target(job: &Job, default_label: &str) -> Option<&'static str> {
    let is_scale_set = RunsOn::parse(&job.runs_on).is_ok_and(|selector| selector.is_scale_set());
    (!is_scale_set)
        .then(|| crate::job_runners::cache_target_for_job(job, default_label))
        .flatten()
}

/// True when shell argv invokes the `mbx` program or tool spec.
fn uses_mbx_tool(step: &Step) -> bool {
    matches!(&step.kind, velnor_actions_contract::StepKind::Shell { run, .. } if run.iter().any(|arg| arg == "mbx" || arg.contains("mr-boxington")))
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
    if !matches!(layer, "sources" | "task" | "tools" | "tofu-providers" | "mbx") {
        return Err(RenderError::BadCommand("mbx_needs_objects_mode".to_owned()));
    }
    if !cache_key_shape_ok(key) || key.contains('\n') || key.contains('\r') {
        return Err(RenderError::BadCommand("bad_cache_key".to_owned()));
    }
    crate::expressions::check_with_value("key", key)?;
    for restore_key in restore_keys {
        if !cache_key_shape_ok(restore_key)
            || restore_key.contains('\n')
            || restore_key.contains('\r')
        {
            return Err(RenderError::BadCommand("bad_cache_restore_key".to_owned()));
        }
        crate::expressions::check_with_value("restore-keys", restore_key)?;
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
        || layer == "tofu-providers" && crate::tofu_cache::tofu_providers_path_ok(path)
        || layer == "mbx" && path == crate::stock_mbx_bundle::MBX_BUNDLE_CACHE_PATH
    {
        Ok(())
    } else {
        Err(RenderError::BadCommand(format!("bad_cache_path:{path}")))
    }
}

fn cache_key_shape_ok(value: &str) -> bool {
    if value.is_empty() {
        return false;
    }
    let mut rest = value;
    while let Some(start) = rest.find("${{") {
        if !rest[..start].bytes().all(is_cache_key_literal_byte) {
            return false;
        }
        let Some(end) = rest[start + 3..].find("}}") else {
            return false;
        };
        rest = &rest[start + 3 + end + 2..];
    }
    rest.bytes().all(is_cache_key_literal_byte)
}

fn is_cache_key_literal_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-')
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
    matches!(
        suffix,
        ".crates.toml" | ".crates2.json" | "bin" | "registry/index" | "registry/cache" | "git/db"
    ) || suffix.starts_with("registry/index/")
        || suffix.starts_with("registry/cache/")
        || suffix.starts_with("git/db/")
        || suffix.starts_with("bin/")
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
