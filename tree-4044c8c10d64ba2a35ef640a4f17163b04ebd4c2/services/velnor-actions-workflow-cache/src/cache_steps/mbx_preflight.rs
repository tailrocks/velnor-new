//! Native MBX object-cache action setup and Rust toolchain preflight.

use std::collections::BTreeMap;

use super::{CompileDriver, MBX_ACTION_NAME};
use velnor_actions_contract::cachekey::mbx_cache_generation;
use velnor_actions_contract_workflow::{Step, StepRole};
use velnor_actions_workflow_steps::RenderError;
use velnor_actions_workflow_steps::steps::{action_step_with_env, shell_step, validate_uses};

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
            let preflight = rust_path_preflight_step(rust_toolchain, rust_env.clone())?;
            let mut action_env = rust_env;
            action_env.insert(MBX_CACHE_MODE_ENV.to_owned(), CACHE_MODE_VALUE.to_owned());
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
pub const MBX_GC_AUTO_ENV: &str = "MBX_GC_AUTO";
/// MBX automatic collection remains enabled for the native object store.
pub const MBX_GC_AUTO_VALUE: &str = "1";
/// Disable shared `OUT_DIR` stabilization in each independently mounted store.
pub const MBX_SHARE_OUT_DIR_ENV: &str = "MBX_SHARE_OUT_DIR";
/// Shared `OUT_DIR` stabilization stays off in every mounted store.
pub const MBX_SHARE_OUT_DIR_VALUE: &str = "0";
/// Stable logical store path; runner namespaces provide physical job isolation.
pub(crate) const MBX_CACHE_DIR_ENV: &str = "MBX_CACHE_DIR";
pub(crate) const MBX_CACHE_DIR_VALUE: &str = "${{ runner.temp }}/velnor/mbx";
/// Write permission is granted only to protected default-branch pushes.
const CACHE_MODE_VALUE: &str = "${{ github.event_name == 'push' && github.ref == format('refs/heads/{0}', github.event.repository.default_branch) && github.ref_protected == true && 'write' || 'read' }}";

fn rust_path_preflight_step(
    rust_toolchain: &str,
    env: BTreeMap<String, String>,
) -> Result<Step, RenderError> {
    let script = [
        "set -eu".to_owned(),
        "case \"$RUNNER_TEMP\" in /*) ;; *) printf '%s\\n' 'RUNNER_TEMP must be absolute' >&2; exit 1 ;; esac".to_owned(),
        "[ -d \"$RUNNER_TEMP\" ] || { printf '%s\\n' 'RUNNER_TEMP must exist' >&2; exit 1; }".to_owned(),
        "check_no_symlink() { path=$1; while [ \"$path\" != / ]; do [ ! -L \"$path\" ] || return 1; path=${path%/*}; [ -n \"$path\" ] || path=/; done; return 0; }".to_owned(),
        "check_no_symlink \"$RUNNER_TEMP\" || { printf '%s\\n' 'RUNNER_TEMP path contains a symlink' >&2; exit 1; }".to_owned(),
        "mbx_cache_dir=\"$RUNNER_TEMP/velnor/mbx\"".to_owned(),
        "[ \"${MBX_CACHE_DIR:-}\" = \"$mbx_cache_dir\" ] || { printf '%s\\n' 'MBX_CACHE_DIR must use the stable runner-temp path' >&2; exit 1; }".to_owned(),
        "[ ! -L \"$RUNNER_TEMP/velnor\" ] || { printf '%s\\n' 'Velnor temp root must not be a symlink' >&2; exit 1; }".to_owned(),
        "if [ ! -e \"$RUNNER_TEMP/velnor\" ]; then (umask 077 && mkdir -m 700 \"$RUNNER_TEMP/velnor\"); fi".to_owned(),
        "[ -d \"$RUNNER_TEMP/velnor\" ] && check_no_symlink \"$RUNNER_TEMP/velnor\" || { printf '%s\\n' 'Velnor temp root must be a real directory' >&2; exit 1; }".to_owned(),
        "[ ! -e \"$mbx_cache_dir\" ] && [ ! -L \"$mbx_cache_dir\" ] || { printf '%s\\n' 'MBX store root already exists in this private runner namespace' >&2; exit 1; }".to_owned(),
        "(umask 077 && mkdir -m 700 \"$mbx_cache_dir\") || { printf '%s\\n' 'could not reserve the MBX store root' >&2; exit 1; }".to_owned(),
        "[ -n \"${GITHUB_ENV:-}\" ] && [ -f \"$GITHUB_ENV\" ] && [ ! -L \"$GITHUB_ENV\" ] || { printf '%s\\n' 'GITHUB_ENV must be a regular runner file' >&2; exit 1; }".to_owned(),
        "check_no_symlink \"$GITHUB_ENV\" || { printf '%s\\n' 'GITHUB_ENV path contains a symlink' >&2; exit 1; }".to_owned(),
        "printf 'MBX_CACHE_DIR=%s\\n' \"$mbx_cache_dir\" >> \"$GITHUB_ENV\"".to_owned(),
        "[ -n \"$GITHUB_PATH\" ] || { printf '%s\\n' 'GITHUB_PATH is required' >&2; exit 1; }".to_owned(),
        "case \"$GITHUB_RUN_ID\" in ''|*[!0-9]*) printf '%s\\n' 'run id must be numeric' >&2; exit 1 ;; esac".to_owned(),
        "case \"$GITHUB_RUN_ATTEMPT\" in ''|*[!0-9]*) printf '%s\\n' 'run attempt must be numeric' >&2; exit 1 ;; esac".to_owned(),
        "probe_dir=\"$RUNNER_TEMP/velnor-mbx-preflight-$GITHUB_RUN_ID-$GITHUB_RUN_ATTEMPT\"".to_owned(),
        "[ ! -e \"$probe_dir\" ] && [ ! -L \"$probe_dir\" ] || { printf '%s\\n' 'preflight scratch already exists' >&2; exit 1; }".to_owned(),
        "(umask 077 && mkdir -m 700 \"$probe_dir\") || { printf '%s\\n' 'could not reserve preflight scratch' >&2; exit 1; }".to_owned(),
        "cleanup() { rm -f \"$probe_dir/rust-root\" \"$probe_dir/rustc-output\"; rmdir \"$probe_dir\"; }".to_owned(),
        "trap cleanup EXIT".to_owned(),
        "trap 'exit 1' HUP INT TERM".to_owned(),
        format!("mise --no-config --no-env --no-hooks where 'rust@{rust_toolchain}' > \"$probe_dir/rust-root\""),
        "exec 3< \"$probe_dir/rust-root\"".to_owned(),
        "IFS= read -r rust_root <&3 || { printf '%s\\n' 'Mise returned no Rust install path' >&2; exit 1; }".to_owned(),
        "extra=; if IFS= read -r extra <&3 || [ -n \"$extra\" ]; then printf '%s\\n' 'Mise returned multiple Rust install paths' >&2; exit 1; fi".to_owned(),
        "exec 3<&-".to_owned(),
        "[ -n \"$rust_root\" ] || { printf '%s\\n' 'Mise returned an empty Rust install path' >&2; exit 1; }".to_owned(),
        "case \"$rust_root\" in /*) ;; *) printf '%s\\n' 'Rust install path must be absolute' >&2; exit 1 ;; esac".to_owned(),
        "rustc_bin=\"$rust_root/rustc\"".to_owned(),
        "[ -x \"$rustc_bin\" ] || { printf 'expected Rustup shim at %s\\n' \"$rustc_bin\" >&2; exit 1; }".to_owned(),
        format!("\"$rustc_bin\" '+{rust_toolchain}' -vV > \"$probe_dir/rustc-output\""),
        format!("grep -Fqx 'release: {rust_toolchain}' \"$probe_dir/rustc-output\" || {{ printf 'Rustup did not report exact toolchain {rust_toolchain}\\n' >&2; exit 1; }}"),
        "printf '%s\\n' \"$rust_root\" >> \"$GITHUB_PATH\"".to_owned(),
    ]
    .join("; ");
    let mut step = shell_step(
        MBX_PREFLIGHT_NAME,
        vec!["sh".to_owned(), "-c".to_owned(), script],
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
    let script = [
        "set -eu".to_owned(),
        "case \"$RUNNER_TEMP\" in /*) ;; *) printf '%s\\n' 'RUNNER_TEMP must be absolute' >&2; exit 1 ;; esac".to_owned(),
        "check_no_symlink() { path=$1; while [ \"$path\" != / ]; do [ ! -L \"$path\" ] || return 1; path=${path%/*}; [ -n \"$path\" ] || path=/; done; return 0; }".to_owned(),
        "[ -d \"$RUNNER_TEMP\" ] && check_no_symlink \"$RUNNER_TEMP\" || { printf '%s\\n' 'RUNNER_TEMP must be a real directory' >&2; exit 1; }".to_owned(),
        "[ \"${MBX_CACHE_DIR:-}\" = \"$RUNNER_TEMP/velnor/mbx\" ] || { printf '%s\\n' 'MBX_CACHE_DIR changed from its stable runner-temp path' >&2; exit 1; }".to_owned(),
        "[ -d \"$RUNNER_TEMP/velnor\" ] && [ -d \"$MBX_CACHE_DIR\" ] && [ -d \"$MBX_CACHE_DIR/actions\" ] || { printf '%s\\n' 'MBX store directories are missing' >&2; exit 1; }".to_owned(),
        "check_no_symlink \"$RUNNER_TEMP/velnor\" && check_no_symlink \"$MBX_CACHE_DIR/actions\" || { printf '%s\\n' 'MBX store path contains a symlink' >&2; exit 1; }".to_owned(),
        "case \"$GITHUB_RUN_ID\" in ''|*[!0-9]*) printf '%s\\n' 'run id must be numeric' >&2; exit 1 ;; esac".to_owned(),
        "case \"$GITHUB_RUN_ATTEMPT\" in ''|*[!0-9]*) printf '%s\\n' 'run attempt must be numeric' >&2; exit 1 ;; esac".to_owned(),
        "probe_dir=\"$RUNNER_TEMP/velnor-mbx-verify-$GITHUB_RUN_ID-$GITHUB_RUN_ATTEMPT\"".to_owned(),
        "[ ! -e \"$probe_dir\" ] && [ ! -L \"$probe_dir\" ] || { printf '%s\\n' 'verification scratch already exists' >&2; exit 1; }".to_owned(),
        "(umask 077 && mkdir -m 700 \"$probe_dir\") || { printf '%s\\n' 'could not reserve verification scratch' >&2; exit 1; }".to_owned(),
        "cleanup() { rm -f \"$probe_dir/version\" \"$probe_dir/cache-dir\"; rmdir \"$probe_dir\"; }".to_owned(),
        "trap cleanup EXIT".to_owned(),
        "trap 'exit 1' HUP INT TERM".to_owned(),
        format!("mise --no-config --no-env --no-hooks exec rust@{rust_toolchain} -- mbx --version > \"$probe_dir/version\""),
        "exec 3< \"$probe_dir/version\"".to_owned(),
        "IFS= read -r version_line <&3 || { printf '%s\\n' 'MBX version output is empty or unterminated' >&2; exit 1; }".to_owned(),
        "extra=; if IFS= read -r extra <&3 || [ -n \"$extra\" ]; then printf '%s\\n' 'MBX version output has extra data' >&2; exit 1; fi".to_owned(),
        "exec 3<&-".to_owned(),
        format!("[ \"$version_line\" = 'mbx {mbx_version}' ] || {{ printf '%s\\n' 'MBX on PATH does not match the pinned action version' >&2; exit 1; }}"),
        format!("mise --no-config --no-env --no-hooks exec rust@{rust_toolchain} -- mbx cache dir > \"$probe_dir/cache-dir\""),
        "exec 3< \"$probe_dir/cache-dir\"".to_owned(),
        "IFS= read -r cache_dir <&3 || { printf '%s\\n' 'MBX cache dir output is empty or unterminated' >&2; exit 1; }".to_owned(),
        "extra=; if IFS= read -r extra <&3 || [ -n \"$extra\" ]; then printf '%s\\n' 'MBX cache dir output has extra data' >&2; exit 1; fi".to_owned(),
        "exec 3<&-".to_owned(),
        "[ \"$cache_dir\" = \"$MBX_CACHE_DIR/actions\" ] || { printf '%s\\n' 'MBX reported an unexpected object store path' >&2; exit 1; }".to_owned(),
    ]
    .join("; ");
    let mut step = shell_step(
        MBX_VERSION_CHECK_NAME,
        vec!["sh".to_owned(), "-c".to_owned(), script],
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
    velnor_actions_workflow_steps::toolchain_env::check_toolchain_homes(&env)?;
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
