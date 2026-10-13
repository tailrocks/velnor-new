//! Native MBX object-cache action setup and Rust toolchain preflight.

use std::collections::BTreeMap;

use super::{CompileDriver, MBX_ACTION_NAME};
use crate::RenderError;
use crate::steps::{action_step_with_env, shell_step, validate_uses};
use velnor_actions_contract::cachekey::mbx_cache_generation;
use velnor_actions_contract::{Step, StepRole};

/// Display name for the strict Rust check before the action installs MBX.
pub const MBX_PREFLIGHT_NAME: &str = "Verify Rust before MBX action";

/// Rust preflight, native cache action, and exact PATH version check; Cargo yields none.
///
/// The action installs the exact MBX release itself. The preceding preflight
/// validates only the selected Rustup toolchain and exports its owned path.
/// # Errors
pub fn mbx_steps_for_driver(
    uses: &str,
    driver: CompileDriver,
    mbx_version: &str,
    rust_toolchain: &str,
    env: BTreeMap<String, String>,
) -> Result<Option<[Step; 3]>, RenderError> {
    match driver {
        CompileDriver::Cargo => Ok(None),
        CompileDriver::Mbx => {
            validate_rust_toolchain(rust_toolchain)?;
            let mut rust_env = with_rustup_process_homes(env)?;
            rust_env.insert(MBX_CACHE_DIR_ENV.to_owned(), MBX_CACHE_DIR_VALUE.to_owned());
            let mut action_env = rust_env;
            action_env.insert(MBX_CACHE_MODE_ENV.to_owned(), CACHE_MODE_VALUE.to_owned());
            let preflight = canonical_mbx_preflight_step(mbx_version, rust_toolchain, &action_env)?;
            let action =
                mbx_objects_action_step(uses, mbx_version, rust_toolchain, action_env.clone())?;
            let version_check = mbx_version_check_step(mbx_version, rust_toolchain, action_env)?;
            Ok(Some([preflight, action, version_check]))
        }
    }
}

/// Action environment key controlling whether the object cache may write.
pub const MBX_CACHE_MODE_ENV: &str = "ACTIONS_CACHE_MODE";
/// Display name of the native MBX object-cache action.
pub const MBX_RESTORE_NAME: &str = "Restore MBX objects";
/// Display name for the exact MBX binary placed on PATH by its native action.
pub const MBX_VERSION_CHECK_NAME: &str = "Verify native MBX version";
/// Preserve MBX's automatic collection for low-disk recovery.
pub(crate) const MBX_GC_AUTO_ENV: &str = "MBX_GC_AUTO";
/// MBX automatic collection remains enabled for the native object store.
pub(crate) const MBX_GC_AUTO_VALUE: &str = "1";
/// Disable shared `OUT_DIR` stabilization in each independently mounted store.
pub(crate) const MBX_SHARE_OUT_DIR_ENV: &str = "MBX_SHARE_OUT_DIR";
pub(crate) const MBX_SHARE_OUT_DIR_VALUE: &str = "0";
/// Stable logical store path; runner namespaces provide physical job isolation.
pub(crate) const MBX_CACHE_DIR_ENV: &str = "MBX_CACHE_DIR";
pub(crate) const MBX_CACHE_DIR_VALUE: &str = "${{ runner.temp }}/velnor/mbx";
/// Write permission is granted only to protected default-branch pushes.
const CACHE_MODE_VALUE: &str = "${{ github.event_name == 'push' && github.ref == format('refs/heads/{0}', github.event.repository.default_branch) && github.ref_protected == true && 'write' || 'read' }}";

/// Reconstruct the only preflight authorized for the selected MBX action.
///
/// The action environment is the source of truth for the toolchain setup;
/// the cache write gate belongs only to the action and is removed before
/// constructing the shell step.
pub(super) fn canonical_mbx_preflight_step(
    mbx_version: &str,
    rust_toolchain: &str,
    action_env: &BTreeMap<String, String>,
) -> Result<Step, RenderError> {
    if !is_exact_mbx_version(mbx_version) {
        return Err(RenderError::BadCommand(format!(
            "bad_mbx_version:{mbx_version}"
        )));
    }
    validate_rust_toolchain(rust_toolchain)?;
    if action_env.get(MBX_CACHE_MODE_ENV).map(String::as_str) != Some(CACHE_MODE_VALUE) {
        return Err(RenderError::InvalidWorkflow(
            "mbx_cache_mode_mismatch".to_owned(),
        ));
    }
    let mut preflight_env = action_env.clone();
    preflight_env.remove(MBX_CACHE_MODE_ENV);
    if preflight_env.get(MBX_CACHE_DIR_ENV).map(String::as_str) != Some(MBX_CACHE_DIR_VALUE) {
        return Err(RenderError::InvalidWorkflow(
            "mbx_cache_dir_mismatch".to_owned(),
        ));
    }
    if with_rustup_process_homes(preflight_env.clone())? != preflight_env {
        return Err(RenderError::InvalidWorkflow(
            "mbx_toolchain_env_mismatch".to_owned(),
        ));
    }
    rust_path_preflight_step(rust_toolchain, preflight_env)
}

fn rust_path_preflight_step(
    rust_toolchain: &str,
    env: BTreeMap<String, String>,
) -> Result<Step, RenderError> {
    let mut step = shell_step(
        MBX_PREFLIGHT_NAME,
        vec![
            "sh".to_owned(),
            crate::render_cache_files::MBX_RUST_CHECKS_SCRIPT_PATH.to_owned(),
            "preflight".to_owned(),
            rust_toolchain.to_owned(),
        ],
        env,
    )?;
    step.role = Some(StepRole::MbxPreflight);
    Ok(step)
}

pub(super) fn mbx_version_check_step(
    mbx_version: &str,
    rust_toolchain: &str,
    env: BTreeMap<String, String>,
) -> Result<Step, RenderError> {
    if !is_exact_mbx_version(mbx_version) {
        return Err(RenderError::BadCommand(format!(
            "bad_mbx_version:{mbx_version}"
        )));
    }
    validate_rust_toolchain(rust_toolchain)?;
    let mut step = shell_step(
        MBX_VERSION_CHECK_NAME,
        vec![
            "sh".to_owned(),
            crate::render_cache_files::MBX_RUST_CHECKS_SCRIPT_PATH.to_owned(),
            "verify".to_owned(),
            mbx_version.to_owned(),
            rust_toolchain.to_owned(),
        ],
        env,
    )?;
    step.role = Some(StepRole::MbxVersionCheck);
    Ok(step)
}

fn mbx_objects_action_step(
    uses: &str,
    mbx_version: &str,
    rust_toolchain: &str,
    env: BTreeMap<String, String>,
) -> Result<Step, RenderError> {
    validate_uses(uses)?;
    let action_sha = uses
        .strip_prefix(&format!("{MBX_ACTION_NAME}@"))
        .ok_or_else(|| RenderError::BadActionRef(format!("not_mbx_action:{uses}")))?;
    if action_sha.len() != 40
        || !action_sha
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(RenderError::BadActionRef(format!(
            "not_pinned_mbx_action:{uses}"
        )));
    }
    if !is_exact_mbx_version(mbx_version) {
        return Err(RenderError::BadCommand(format!(
            "bad_mbx_version:{mbx_version}"
        )));
    }
    validate_rust_toolchain(rust_toolchain)?;
    let with = BTreeMap::from([
        ("github-cache-mode".to_owned(), "objects".to_owned()),
        ("version".to_owned(), mbx_version.to_owned()),
        ("toolchain".to_owned(), rust_toolchain.to_owned()),
        (
            "cache-generation".to_owned(),
            cache_generation(mbx_version, action_sha),
        ),
        ("save-on-workflow-dispatch".to_owned(), "false".to_owned()),
        ("save-on-pull-request".to_owned(), "false".to_owned()),
        ("save-on-protected-branch".to_owned(), "false".to_owned()),
    ]);
    let mut step = action_step_with_env(MBX_RESTORE_NAME, uses, with, env)?;
    step.role = Some(StepRole::MbxCache);
    Ok(step)
}

fn cache_generation(mbx_version: &str, action_sha: &str) -> String {
    // These action-input contexts identify the actual runner and job, even
    // when hosted-only and Scale Set-only plans use the same logical job ID.
    format!(
        "{}-gc-auto-v1-action-{action_sha}-lane-${{{{ runner.environment }}}}-job-${{{{ github.job }}}}",
        mbx_cache_generation(mbx_version)
    )
}

fn with_rustup_process_homes(
    mut env: BTreeMap<String, String>,
) -> Result<BTreeMap<String, String>, RenderError> {
    crate::toolchain_env::check_toolchain_homes(&env)?;
    let rustup_home = env
        .get("MISE_RUSTUP_HOME")
        .filter(|value| !value.is_empty())
        .cloned()
        .ok_or_else(|| RenderError::BadCommand("missing_mise_rustup_home".to_owned()))?;
    let cargo_home = env
        .get("MISE_CARGO_HOME")
        .filter(|value| !value.is_empty())
        .cloned()
        .ok_or_else(|| RenderError::BadCommand("missing_mise_cargo_home".to_owned()))?;
    env.insert("RUSTUP_HOME".to_owned(), rustup_home);
    env.insert("CARGO_HOME".to_owned(), cargo_home);
    Ok(env)
}

fn is_exact_mbx_version(version: &str) -> bool {
    let parts: Vec<&str> = version.split('.').collect();
    parts.len() == 3
        && parts
            .iter()
            .all(|part| !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit()))
}

fn validate_rust_toolchain(toolchain: &str) -> Result<(), RenderError> {
    let parts: Vec<&str> = toolchain.split('.').collect();
    if parts.len() == 3
        && parts
            .iter()
            .all(|part| !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit()))
    {
        Ok(())
    } else {
        Err(RenderError::BadCommand(format!(
            "bad_rust_toolchain:{toolchain}"
        )))
    }
}
