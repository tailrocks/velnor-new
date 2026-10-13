//! Native MBX object-cache action setup and Rust toolchain preflight.

use std::collections::BTreeMap;

use super::mbx_command::{has_external_mbx_selector, uses_mbx_command};
use super::{CompileDriver, MBX_ACTION_NAME};
use crate::RenderError;
use crate::steps::{action_step_with_env, shell_step, validate_uses};
use velnor_actions_contract::cachekey::mbx_cache_generation;
use velnor_actions_contract::{Job, Step};

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
            let rust_env = with_rustup_process_homes(env)?;
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

/// Gate native MBX action and executable selection against each job's driver.
///
/// Jobs without a declared driver are skipped (plan/final/lint carry none);
/// Cargo jobs are MBX-free while MBX jobs carry exactly one action and command.
/// # Errors
pub fn check_mbx_gating(
    jobs: &BTreeMap<String, Job>,
    drivers: &BTreeMap<String, CompileDriver>,
) -> Result<(), RenderError> {
    for (id, job) in jobs {
        if job.steps.iter().any(has_external_mbx_selector) {
            return Err(RenderError::InvalidWorkflow(format!(
                "mbx_mise_selector_forbidden:{id}"
            )));
        }
    }
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

fn check_job_mbx(id: &str, job: &Job, driver: CompileDriver) -> Result<(), RenderError> {
    let actions: Vec<usize> = job
        .steps
        .iter()
        .enumerate()
        .filter(|(_, step)| is_mbx_action(step))
        .map(|(index, _)| index)
        .collect();
    let all_commands: Vec<usize> = job
        .steps
        .iter()
        .enumerate()
        .filter(|(_, step)| uses_mbx_command(step) && !is_mbx_action(step))
        .map(|(index, _)| index)
        .collect();
    let build_commands: Vec<usize> = job
        .steps
        .iter()
        .enumerate()
        .filter(|(_, step)| {
            step.name != MBX_VERSION_CHECK_NAME && uses_mbx_command(step) && !is_mbx_action(step)
        })
        .map(|(index, _)| index)
        .collect();
    let version_checks: Vec<usize> = job
        .steps
        .iter()
        .enumerate()
        .filter(|(_, step)| step.name == MBX_VERSION_CHECK_NAME)
        .map(|(index, _)| index)
        .collect();
    match driver {
        CompileDriver::Cargo if !actions.is_empty() => Err(RenderError::InvalidWorkflow(format!(
            "mbx_action_without_selection:{id}"
        ))),
        CompileDriver::Cargo if !all_commands.is_empty() => Err(RenderError::InvalidWorkflow(
            format!("mbx_tool_without_selection:{id}"),
        )),
        CompileDriver::Cargo => Ok(()),
        CompileDriver::Mbx if actions.is_empty() => Err(RenderError::InvalidWorkflow(format!(
            "mbx_missing_for_selection:{id}"
        ))),
        CompileDriver::Mbx if actions.len() > 1 => {
            Err(RenderError::InvalidWorkflow(format!("mbx_duplicated:{id}")))
        }
        CompileDriver::Mbx if version_checks.is_empty() => Err(RenderError::InvalidWorkflow(
            format!("mbx_version_check_missing:{id}"),
        )),
        CompileDriver::Mbx if version_checks.len() > 1 => Err(RenderError::InvalidWorkflow(
            format!("mbx_version_check_duplicated:{id}"),
        )),
        CompileDriver::Mbx => {
            check_mbx_version_order(id, job, actions[0], &build_commands, version_checks[0])
        }
    }
}

fn check_mbx_version_order(
    id: &str,
    job: &Job,
    action_at: usize,
    commands: &[usize],
    check_at: usize,
) -> Result<(), RenderError> {
    let Some(action) = job.steps.get(action_at) else {
        return Err(RenderError::InvalidWorkflow(format!(
            "mbx_action_missing:{id}"
        )));
    };
    let velnor_actions_contract::StepKind::Action { with, env, .. } = &action.kind else {
        return Err(RenderError::InvalidWorkflow(format!(
            "mbx_action_invalid:{id}"
        )));
    };
    let Some(version) = with.get("version") else {
        return Err(RenderError::InvalidWorkflow(format!(
            "mbx_version_missing:{id}"
        )));
    };
    let Some(rust_toolchain) = with.get("toolchain") else {
        return Err(RenderError::InvalidWorkflow(format!(
            "mbx_toolchain_missing:{id}"
        )));
    };
    let expected = mbx_version_check_step(version, rust_toolchain, env.clone())?;
    if job.steps.get(check_at) != Some(&expected) {
        return Err(RenderError::InvalidWorkflow(format!(
            "mbx_version_check_mismatch:{id}"
        )));
    }
    if action_at >= check_at
        || commands.is_empty()
        || commands.iter().any(|index| *index <= check_at)
    {
        return Err(RenderError::InvalidWorkflow(format!(
            "mbx_version_check_order:{id}"
        )));
    }
    Ok(())
}

/// True for the pinned native MBX action.
pub(crate) fn is_mbx_action(step: &Step) -> bool {
    matches!(&step.kind, velnor_actions_contract::StepKind::Action { uses, .. } if uses.starts_with(&format!("{MBX_ACTION_NAME}@")))
}

/// Action environment key controlling whether the object cache may write.
pub const MBX_CACHE_MODE_ENV: &str = "ACTIONS_CACHE_MODE";
/// Display name of the native MBX object-cache action.
pub const MBX_RESTORE_NAME: &str = "Restore MBX objects";
/// Display name for the exact MBX binary placed on PATH by its native action.
pub const MBX_VERSION_CHECK_NAME: &str = "Verify native MBX version";
/// Preserve MBX's automatic collection for low-disk recovery.
pub(crate) const MBX_GC_AUTO_ENV: &str = "MBX_GC_AUTO";
/// MBX automatic collection remains enabled.
pub(crate) const MBX_GC_AUTO_VALUE: &str = "1";
/// Disable shared `OUT_DIR` stabilization so deleted stores remain removable.
pub(crate) const MBX_SHARE_OUT_DIR_ENV: &str = "MBX_SHARE_OUT_DIR";
/// Keep the shared output-directory optimization disabled in every MBX lane.
pub(crate) const MBX_SHARE_OUT_DIR_VALUE: &str = "0";
/// Write permission is granted only to protected default-branch pushes.
const CACHE_MODE_VALUE: &str = "${{ github.event_name == 'push' && github.ref == format('refs/heads/{0}', github.event.repository.default_branch) && github.ref_protected == true && 'write' || 'read' }}";
/// The private object store is for disposable hosted runners only.
const HOSTED_RUNNER: &str = "${{ runner.environment == 'github-hosted' }}";

fn rust_path_preflight_step(
    rust_toolchain: &str,
    env: BTreeMap<String, String>,
) -> Result<Step, RenderError> {
    let script = [
        "set -eu".to_owned(),
        "[ -n \"$RUNNER_TEMP\" ] && [ -d \"$RUNNER_TEMP\" ] && [ ! -L \"$RUNNER_TEMP\" ] || { printf '%s\\n' 'RUNNER_TEMP must be an owned directory' >&2; exit 1; }".to_owned(),
        "[ -n \"$GITHUB_PATH\" ] || { printf '%s\\n' 'GITHUB_PATH is required' >&2; exit 1; }"
            .to_owned(),
        "case \"$GITHUB_RUN_ID\" in ''|*[!0-9]*) printf '%s\\n' 'GITHUB_RUN_ID must be numeric' >&2; exit 1 ;; esac".to_owned(),
        "case \"$GITHUB_RUN_ATTEMPT\" in ''|*[!0-9]*) printf '%s\\n' 'GITHUB_RUN_ATTEMPT must be numeric' >&2; exit 1 ;; esac".to_owned(),
        "probe_dir=\"$RUNNER_TEMP/velnor-mbx-preflight-$GITHUB_RUN_ID-$GITHUB_RUN_ATTEMPT\"".to_owned(),
        "(umask 077 && mkdir \"$probe_dir\") || { printf '%s\\n' 'preflight scratch already exists' >&2; exit 1; }".to_owned(),
        "cleanup() { rm -f \"$probe_dir/rust-root\" \"$probe_dir/rustc-output\"; rmdir \"$probe_dir\"; }".to_owned(),
        "trap cleanup EXIT".to_owned(),
        "trap 'exit 1' HUP INT TERM".to_owned(),
        format!("mise --no-config --no-env --no-hooks where 'rust@{rust_toolchain}' > \"$probe_dir/rust-root\""),
        "exec 3< \"$probe_dir/rust-root\"".to_owned(),
        "IFS= read -r rust_root <&3 || { printf '%s\\n' 'Mise returned no Rust install path' >&2; exit 1; }".to_owned(),
        "extra=; if IFS= read -r extra <&3 || [ -n \"$extra\" ]; then printf '%s\\n' 'Mise returned multiple Rust install paths' >&2; exit 1; fi".to_owned(),
        "exec 3<&-".to_owned(),
        "[ -n \"$rust_root\" ] || { printf '%s\\n' 'Mise returned an empty Rust install path' >&2; exit 1; }"
            .to_owned(),
        "case \"$rust_root\" in /*) ;; *) printf '%s\\n' 'Mise returned a non-absolute Rust install path' >&2; exit 1 ;; esac"
            .to_owned(),
        "rustc_bin=\"$rust_root/rustc\"".to_owned(),
        "[ -x \"$rustc_bin\" ] || { printf 'expected Rustup shim at %s\\n' \"$rustc_bin\" >&2; exit 1; }"
            .to_owned(),
        format!("\"$rustc_bin\" '+{rust_toolchain}' -vV > \"$probe_dir/rustc-output\""),
        format!(
            "grep -Fqx 'release: {rust_toolchain}' \"$probe_dir/rustc-output\" || {{ printf 'Rustup did not report exact toolchain {rust_toolchain}\\n' >&2; exit 1; }}"
        ),
        "printf '%s\\n' \"$rust_root\" >> \"$GITHUB_PATH\"".to_owned(),
    ]
    .join("; ");
    shell_step(
        MBX_PREFLIGHT_NAME,
        vec!["sh".to_owned(), "-c".to_owned(), script],
        env,
    )
}

fn mbx_version_check_step(
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
        "[ -n \"$RUNNER_TEMP\" ] && [ -d \"$RUNNER_TEMP\" ] && [ ! -L \"$RUNNER_TEMP\" ] || { printf '%s\\n' 'RUNNER_TEMP must be an owned directory' >&2; exit 1; }".to_owned(),
        "case \"$GITHUB_RUN_ID\" in ''|*[!0-9]*) printf '%s\\n' 'GITHUB_RUN_ID must be numeric' >&2; exit 1 ;; esac".to_owned(),
        "case \"$GITHUB_RUN_ATTEMPT\" in ''|*[!0-9]*) printf '%s\\n' 'GITHUB_RUN_ATTEMPT must be numeric' >&2; exit 1 ;; esac".to_owned(),
        "case \"$GITHUB_JOB\" in ''|*[!A-Za-z0-9_-]*) printf '%s\\n' 'GITHUB_JOB must be an identifier' >&2; exit 1 ;; esac".to_owned(),
        "verify_dir=\"$RUNNER_TEMP/velnor-mbx-version-$GITHUB_RUN_ID-$GITHUB_RUN_ATTEMPT-$GITHUB_JOB\"".to_owned(),
        "(umask 077 && mkdir \"$verify_dir\") || { printf '%s\\n' 'version-check scratch already exists' >&2; exit 1; }".to_owned(),
        "cleanup() { rm -f \"$verify_dir/version\"; rmdir \"$verify_dir\"; }".to_owned(),
        "trap cleanup EXIT".to_owned(),
        "trap 'exit 1' HUP INT TERM".to_owned(),
        format!("mise --no-config --no-env --no-hooks exec rust@{rust_toolchain} -- mbx --version > \"$verify_dir/version\""),
        "exec 3< \"$verify_dir/version\"".to_owned(),
        "IFS= read -r version_line <&3 || { printf '%s\\n' 'MBX version output is empty or unterminated' >&2; exit 1; }".to_owned(),
        "extra=; if IFS= read -r extra <&3 || [ -n \"$extra\" ]; then printf '%s\\n' 'MBX version output has extra data' >&2; exit 1; fi".to_owned(),
        "exec 3<&-".to_owned(),
        format!("[ \"$version_line\" = 'mbx {mbx_version}' ] || {{ printf '%s\\n' 'MBX on PATH does not match the pinned action version' >&2; exit 1; }}"),
    ]
    .join("; ");
    shell_step(
        MBX_VERSION_CHECK_NAME,
        vec!["sh".to_owned(), "-c".to_owned(), script],
        env,
    )
}

fn mbx_objects_action_step(
    uses: &str,
    mbx_version: &str,
    rust_toolchain: &str,
    env: BTreeMap<String, String>,
) -> Result<Step, RenderError> {
    validate_uses(uses)?;
    if !uses.starts_with(&format!("{MBX_ACTION_NAME}@")) {
        return Err(RenderError::BadActionRef(format!("not_mbx_action:{uses}")));
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
            "cache-key-suffix".to_owned(),
            "${{ github.job }}".to_owned(),
        ),
        (
            "cache-generation".to_owned(),
            mbx_cache_generation(mbx_version),
        ),
        ("isolate-objects-cache".to_owned(), HOSTED_RUNNER.to_owned()),
        ("save-on-workflow-dispatch".to_owned(), "false".to_owned()),
        ("save-on-pull-request".to_owned(), "false".to_owned()),
        ("save-on-protected-branch".to_owned(), "false".to_owned()),
    ]);
    action_step_with_env(MBX_RESTORE_NAME, uses, with, env)
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
