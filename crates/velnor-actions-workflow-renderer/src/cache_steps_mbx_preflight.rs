// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use super::{CompileDriver, MBX_ACTION_NAME};
use crate::RenderError;
use crate::steps::{action_owned_mbx_shell_step, action_step_with_env, shell_step, validate_uses};
use velnor_actions_contract::workflow::MBX_WORKSPACE_CLEAN_CONDITION;
use velnor_actions_contract::{Step, StepId, StepKind, StepRole};

pub const MBX_PREFLIGHT_NAME: &str = "Verify Rust before MBX action";

pub fn mbx_steps_for_driver(
    uses: &str,
    driver: CompileDriver,
    mbx_version: &str,
    rust_toolchain: &str,
    env: BTreeMap<String, String>,
    transport: MbxTransport,
    working_directory: Option<&str>,
) -> Result<Option<Vec<Step>>, RenderError> {
    match driver {
        CompileDriver::Cargo => Ok(None),
        CompileDriver::Mbx => {
            validate_rust_toolchain(rust_toolchain)?;
            let mut action_env = with_rustup_process_homes(env)?;
            action_env.insert(MBX_CACHE_DIR_ENV.to_owned(), MBX_CACHE_DIR_VALUE.to_owned());
            let preflight = canonical_mbx_preflight_step(mbx_version, rust_toolchain, &action_env)?;
            let action = mbx_install_action_step(
                uses,
                mbx_version,
                rust_toolchain,
                action_env.clone(),
                working_directory,
            )?;
            let version_check =
                mbx_version_check_step(mbx_version, rust_toolchain, action_env.clone())?;
            let mut steps = vec![preflight, action, version_check];
            if let Some(unit) = transport.unit() {
                steps.extend(super::mbx_bundle::mbx_bundle_prelude(
                    unit,
                    mbx_version,
                    rust_toolchain,
                    rust_toolchain,
                    working_directory,
                    &action_env,
                )?);
            }
            Ok(Some(steps))
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MbxTransport {
    InstallOnly,
    SeedRead,
    SeedWrite,
    SuiteBundle,
}

impl MbxTransport {
    pub(crate) const fn unit(self) -> Option<&'static str> {
        match self {
            Self::InstallOnly => None,
            Self::SeedRead | Self::SeedWrite => Some("seed"),
            Self::SuiteBundle => Some("suite"),
        }
    }
}

pub const MBX_RESTORE_NAME: &str = "Set up MBX";
pub const MBX_VERSION_CHECK_NAME: &str = "Verify native MBX version";
pub(crate) use crate::action_mbx_shell::{ACTION_MBX_BIN, RESOLVE_ACTION_MBX_BIN};
pub(crate) const MBX_GC_AUTO_ENV: &str = "MBX_GC_AUTO";
pub(crate) const MBX_GC_AUTO_VALUE: &str = "0";
pub(crate) const MBX_SHARE_OUT_DIR_ENV: &str = "MBX_SHARE_OUT_DIR";
pub(crate) const MBX_SHARE_OUT_DIR_VALUE: &str = "0";
pub(crate) const MBX_CC_ENV: &str = "MBX_CC";
pub(crate) const MBX_CC_VALUE: &str = "0";
pub(crate) const MBX_BUILD_SCRIPT_EXECUTION_ENV: &str = "MBX_BUILD_SCRIPT_EXECUTION";
pub(crate) const MBX_BUILD_SCRIPT_EXECUTION_VALUE: &str = "0";
pub(crate) const MBX_CACHE_DIR_ENV: &str = "MBX_CACHE_DIR";
pub(crate) const MBX_CACHE_DIR_VALUE: &str = "${{ runner.temp }}/velnor/mbx";

#[path = "cache_steps_mbx_rustc.rs"]
mod rustc_identity;
#[path = "cache_steps_mbx_scripts.rs"]
mod scripts;
use self::scripts::{mbx_disk_sample_commands, mbx_disk_sample_function};
pub(super) fn canonical_mbx_preflight_step(
    mbx_version: &str,
    rust_toolchain: &str,
    action_env: &BTreeMap<String, String>,
) -> Result<Step, RenderError> {
    if !crate::commands::is_exact_or_project_version(mbx_version) {
        return Err(RenderError::BadCommand(format!(
            "bad_mbx_version:{mbx_version}"
        )));
    }
    validate_rust_toolchain(rust_toolchain)?;
    let preflight_env = action_env.clone();
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
    let mut script = vec![
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
        "(umask 077 && mkdir -m 700 \"$mbx_cache_dir\" && mkdir -m 700 \"$mbx_cache_dir/actions\") || { printf '%s\\n' 'could not reserve the MBX store root' >&2; exit 1; }".to_owned(),
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
    ];
    script.extend(mbx_disk_sample_commands("before-action-restore"));
    let script = script.join("; ");
    let mut step = shell_step(
        MBX_PREFLIGHT_NAME,
        vec!["sh".to_owned(), "-c".to_owned(), script],
        env,
    )?;
    step.role = Some(StepRole::MbxPreflight);
    Ok(step)
}

pub(crate) fn mbx_version_check_step(
    mbx_version: &str,
    rust_toolchain: &str,
    env: BTreeMap<String, String>,
) -> Result<Step, RenderError> {
    if !crate::commands::is_exact_or_project_version(mbx_version) {
        return Err(RenderError::BadCommand(format!(
            "bad_mbx_version:{mbx_version}"
        )));
    }
    validate_rust_toolchain(rust_toolchain)?;
    let mut script = vec![
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
        "cleanup() { rm -f \"$probe_dir/version\" \"$probe_dir/cache-dir\" \"$probe_dir/rustc-path\" \"$probe_dir/rustc-vv\" \"$probe_dir/mbx-rustc-vv\" \"$probe_dir/mbx-rustc\"; rmdir \"$probe_dir\"; }".to_owned(),
        "trap cleanup EXIT".to_owned(),
        "trap 'exit 1' HUP INT TERM".to_owned(),
        RESOLVE_ACTION_MBX_BIN.to_owned(),
        format!("mise --no-config --no-env --no-hooks exec rust@{rust_toolchain} -- \"{ACTION_MBX_BIN}\" --version > \"$probe_dir/version\""),
        "exec 3< \"$probe_dir/version\"".to_owned(),
        "IFS= read -r version_line <&3 || { printf '%s\\n' 'MBX version output is empty or unterminated' >&2; exit 1; }".to_owned(),
        "extra=; if IFS= read -r extra <&3 || [ -n \"$extra\" ]; then printf '%s\\n' 'MBX version output has extra data' >&2; exit 1; fi".to_owned(),
        "exec 3<&-".to_owned(),
        format!("[ \"$version_line\" = 'mbx {mbx_version}' ] || {{ printf '%s\\n' 'MBX on PATH does not match the pinned action version' >&2; exit 1; }}"),
        format!("mise --no-config --no-env --no-hooks exec rust@{rust_toolchain} -- \"{ACTION_MBX_BIN}\" cache dir > \"$probe_dir/cache-dir\""),
        "exec 3< \"$probe_dir/cache-dir\"".to_owned(),
        "IFS= read -r cache_dir <&3 || { printf '%s\\n' 'MBX cache dir output is empty or unterminated' >&2; exit 1; }".to_owned(),
        "extra=; if IFS= read -r extra <&3 || [ -n \"$extra\" ]; then printf '%s\\n' 'MBX cache dir output has extra data' >&2; exit 1; fi".to_owned(),
        "exec 3<&-".to_owned(),
        "[ \"$cache_dir\" = \"$MBX_CACHE_DIR/actions\" ] || { printf '%s\\n' 'MBX reported an unexpected object store path' >&2; exit 1; }".to_owned(),
    ];
    script.extend(rustc_identity::mbx_rustc_identity_commands(rust_toolchain));
    script.extend(mbx_disk_sample_commands("after-cache-restore"));
    let script = script.join("; ");
    let mut step = action_owned_mbx_shell_step(
        MBX_VERSION_CHECK_NAME,
        vec!["sh".to_owned(), "-c".to_owned(), script],
        env,
    )?;
    step.id = Some(StepId::MbxReady);
    step.role = Some(StepRole::MbxVersionCheck);
    Ok(step)
}

pub(crate) fn mbx_workspace_clean_step_for_action(action: &Step) -> Result<Step, RenderError> {
    if action.role != Some(StepRole::MbxCache) {
        return Err(RenderError::InvalidWorkflow(
            "mbx_cleanup_action_role_mismatch".to_owned(),
        ));
    }
    let StepKind::Action { with, env, .. } = &action.kind else {
        return Err(RenderError::InvalidWorkflow(
            "mbx_cleanup_action_kind_mismatch".to_owned(),
        ));
    };
    let version = with
        .get("version")
        .ok_or_else(|| RenderError::InvalidWorkflow("mbx_cleanup_version_missing".to_owned()))?;
    let rust_toolchain = with
        .get("toolchain")
        .ok_or_else(|| RenderError::InvalidWorkflow("mbx_cleanup_toolchain_missing".to_owned()))?;
    if !crate::commands::is_exact_or_project_version(version) {
        return Err(RenderError::BadCommand(format!(
            "bad_mbx_version:{version}"
        )));
    }
    validate_rust_toolchain(rust_toolchain)?;
    let target_dir = crate::action_mbx_shell::target_dir_for_action(with)?;

    let clean_env = env.clone();
    let script = [
        "set -eu".to_owned(),
        "check_no_symlink() { path=$1; while [ \"$path\" != / ]; do [ ! -L \"$path\" ] || return 1; path=${path%/*}; [ -n \"$path\" ] || path=/; done; return 0; }".to_owned(),
        "case \"$RUNNER_TEMP\" in /*) ;; *) printf '%s\\n' 'RUNNER_TEMP must be absolute' >&2; exit 1 ;; esac".to_owned(),
        "[ -d \"$RUNNER_TEMP\" ] && check_no_symlink \"$RUNNER_TEMP\" || { printf '%s\\n' 'RUNNER_TEMP must be a real directory' >&2; exit 1; }".to_owned(),
        "[ \"${MBX_CACHE_DIR:-}\" = \"$RUNNER_TEMP/velnor/mbx\" ] && [ -d \"$RUNNER_TEMP/velnor\" ] && [ -d \"$MBX_CACHE_DIR/actions\" ] && check_no_symlink \"$RUNNER_TEMP/velnor\" && check_no_symlink \"$MBX_CACHE_DIR/actions\" || { printf '%s\\n' 'MBX store path changed before cleanup' >&2; exit 1; }".to_owned(),
        mbx_disk_sample_function(),
        format!("target_dir=\"{target_dir}\""),
        "mbx_disk_sample before-final-clean".to_owned(),
        RESOLVE_ACTION_MBX_BIN.to_owned(),
        format!("clean_status=0; if mise --no-config --no-env --no-hooks exec rust@{rust_toolchain} -- \"{ACTION_MBX_BIN}\" clean; then clean_status=0; else clean_status=$?; fi"),
        "mbx_disk_sample after-final-clean".to_owned(),
        "exit \"$clean_status\"".to_owned(),
    ]
    .join("; ");
    let mut step = action_owned_mbx_shell_step(
        "Clean MBX workspace outputs",
        vec!["sh".to_owned(), "-c".to_owned(), script],
        clean_env,
    )?;
    step.condition = Some(MBX_WORKSPACE_CLEAN_CONDITION.to_owned());
    step.role = Some(StepRole::MbxWorkspaceCleanup);
    Ok(step)
}

fn mbx_install_action_step(
    uses: &str,
    mbx_version: &str,
    rust_toolchain: &str,
    env: BTreeMap<String, String>,
    working_directory: Option<&str>,
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
    if !crate::commands::is_exact_or_project_version(mbx_version) {
        return Err(RenderError::BadCommand(format!(
            "bad_mbx_version:{mbx_version}"
        )));
    }
    validate_rust_toolchain(rust_toolchain)?;
    let mut with = BTreeMap::from([
        ("backend".to_owned(), "local".to_owned()),
        ("version".to_owned(), mbx_version.to_owned()),
        ("toolchain".to_owned(), rust_toolchain.to_owned()),
    ]);
    if let Some(dir) = working_directory
        && !dir.is_empty()
        && dir != "."
    {
        with.insert("working-directory".to_owned(), dir.to_owned());
    }
    let mut step = action_step_with_env(MBX_RESTORE_NAME, uses, with, env)?;
    step.role = Some(StepRole::MbxCache);
    Ok(step)
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

pub(super) fn validate_rust_toolchain(toolchain: &str) -> Result<(), RenderError> {
    crate::commands::is_exact_or_project_version(toolchain)
        .then_some(())
        .ok_or_else(|| RenderError::BadCommand(format!("bad_rust_toolchain:{toolchain}")))
}
