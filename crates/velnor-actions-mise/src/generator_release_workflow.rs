//! Pinned Mise setup and Rust-execution steps for generator release builds.

use std::collections::BTreeMap;
use std::ffi::{OsStr, OsString};

use velnor_actions_contract::{GeneratorReleaseTarget, Step, StepKind};

use crate::catalog::{MISE_VERSION, PinnedTool, ToolCatalog};
use crate::error::MiseError;
use crate::requests::PinnedToolExec;
use crate::steps::{PreparePinnedTools, ToolHomes};

const MISE_ACTION_PREFIX: &str = "jdx/mise-action@";
const SETUP_MISE_STEP: &str = "Setup pinned Mise";
const INSTALL_RUST_MBX_STEP: &str = "Install pinned Rust and MBX";
const CARGO_PROGRAM: &str = "cargo";
const CARGO_BUILD_COMMAND: &str = "build";
const MBX_PROGRAM: &str = "mbx";

/// Installed GNU/Linux x64 Mise binary SHA used by the pinned setup action.
/// It was independently measured from both archive variants in immutable
/// release v2026.9.18; the release API's `.tar.gz` and `.tar.zst` archive
/// digests are `4312f8fd72a8d6a869cd2aca7444929e2a0ef6f45d2c6f2866a1eacc5bdc2e84`
/// and `8c6d5f3a5e94f8270e45935359dba85f14c702c41afd42321efaadd5028ee3c7`.
const MISE_BINARY_SHA256_LINUX_X64: &str =
    "d24fe0bf7e613824ad99f7b8dac3f2b381a37b9f75f84dd250855217095a8de4";
/// Installed macOS ARM64 Mise binary SHA measured from both immutable release
/// v2026.9.18 archive variants. Their release API archive digests are
/// `b3539de1a9823505269481b71d09a9cf86e141c63ebfc6101b3cf561766a83e8` and
/// `e0089ffb8833fb57b862b89f5aae56766a514eaef08eb2bc10ad652d7b10647b`.
const MISE_BINARY_SHA256_MACOS_ARM64: &str =
    "484c135bd4329975d608d3f77e26c2ece5d2f5590f18ca71f44440294f8cfa6f";
/// Installed macOS x64 Mise binary SHA from the pinned x64 platform record.
const MISE_BINARY_SHA256_MACOS_X64: &str =
    "02d8ba561847f996925e361262c0610a24f59fcd9e06ba9ed0b6022e19b317c3";
#[path = "generator_release_workflow_checks.rs"]
mod checks;
pub use checks::{
    apple_linker_check_step, apple_sdk_check_step, binary_format_architecture_check_step,
    gnu_runtime_abi_check_step, help_smoke_check_step, native_host_check_step,
    rust_toolchain_check_step, version_smoke_check_step,
};

#[cfg(test)]
#[path = "generator_release_workflow_tests.rs"]
mod tests;

/// Lower the validated Mise action pin and exact Rust and MBX installs to workflow steps.
///
/// # Errors
///
/// Returns [`MiseError`] for a malformed action pin or invalid pinned install.
pub fn setup_rust_steps(
    mise_action_uses: &str,
    target: GeneratorReleaseTarget,
    mise_sha256: &str,
    homes: &ToolHomes,
    catalog: &ToolCatalog,
) -> Result<Vec<Step>, MiseError> {
    validate_mise_action_uses(mise_action_uses)?;
    if !velnor_actions_contract::ids::is_lower_hex_len(mise_sha256, 64) {
        return Err(invalid_step_input("mise_sha256", mise_sha256));
    }
    if mise_sha256 != generator_release_mise_binary_sha256(target) {
        return Err(invalid_step_input(
            "mise_sha256_target_mismatch",
            mise_sha256,
        ));
    }
    let action = Step {
        name: SETUP_MISE_STEP.to_owned(),
        condition: None,
        kind: StepKind::Action {
            uses: mise_action_uses.to_owned(),
            with: BTreeMap::from([
                ("cache".to_owned(), "false".to_owned()),
                ("cache_save".to_owned(), "false".to_owned()),
                ("env".to_owned(), "false".to_owned()),
                ("install".to_owned(), "false".to_owned()),
                ("sha256".to_owned(), mise_sha256.to_owned()),
                ("version".to_owned(), MISE_VERSION.to_owned()),
            ]),
            env: BTreeMap::new(),
        },
    };
    let install = PreparePinnedTools::new(
        vec![PinnedTool::Rust, PinnedTool::MrBoxington],
        homes.clone(),
    )?;
    let install = shell_step(
        INSTALL_RUST_MBX_STEP,
        install.argv(catalog),
        install.env(catalog),
    )?;
    Ok(vec![action, install])
}

/// Exact installed-binary SHA-256 for the pinned Mise release on one target.
#[must_use]
pub const fn generator_release_mise_binary_sha256(target: GeneratorReleaseTarget) -> &'static str {
    match target {
        GeneratorReleaseTarget::LinuxX86_64 => MISE_BINARY_SHA256_LINUX_X64,
        GeneratorReleaseTarget::MacosArm64 => MISE_BINARY_SHA256_MACOS_ARM64,
        GeneratorReleaseTarget::MacosX86_64 => MISE_BINARY_SHA256_MACOS_X64,
    }
}

/// Lower one Cargo build request to the pinned Rust and MBX Mise tools.
///
/// The input must be exactly `cargo build ...`. The output selects both
/// exact catalog pins and invokes `mbx build ...`; other Cargo commands
/// must use [`rust_exec_step`] when they do not compile source.
///
/// This function creates no process and does not execute the request.
///
/// # Errors
///
/// Returns [`MiseError`] for a malformed name or build payload, or for
/// non-UTF-8 workflow arguments or environment values.
pub fn mbx_cargo_build_step(
    name: &str,
    program: &OsStr,
    args: &[OsString],
    homes: &ToolHomes,
    catalog: &ToolCatalog,
) -> Result<Step, MiseError> {
    if name.trim().is_empty() || name.bytes().any(|byte| byte.is_ascii_control()) {
        return Err(invalid_step_input("name", name));
    }
    if program != OsStr::new(CARGO_PROGRAM) {
        return Err(invalid_step_input("program", &program.to_string_lossy()));
    }
    if args
        .first()
        .is_none_or(|command| command.as_os_str() != OsStr::new(CARGO_BUILD_COMMAND))
    {
        return Err(invalid_step_input("payload", "expected_cargo_build"));
    }
    for argument in args {
        let Some(value) = argument.to_str() else {
            return Err(invalid_step_input("args", "non_utf8"));
        };
        if value.is_empty() {
            return Err(invalid_step_input("args", "empty_argument"));
        }
        if value.chars().any(char::is_control) {
            return Err(invalid_step_input("args", "control_character"));
        }
    }

    let mut mbx_args = Vec::with_capacity(args.len());
    mbx_args.push(OsString::from(CARGO_BUILD_COMMAND));
    mbx_args.extend(args.iter().skip(1).cloned());
    let request = PinnedToolExec::new(
        vec![PinnedTool::Rust, PinnedTool::MrBoxington],
        OsStr::new(MBX_PROGRAM),
        mbx_args,
    )?;
    shell_step(name, request.argv(catalog), homes.exec_env(catalog))
}

/// Lower one pinned Cargo request to a Mise `exec` step.
///
/// This function creates no process and does not execute the request.
///
/// # Errors
///
/// Returns [`MiseError`] for an empty request, forbidden installer payload,
/// invalid name, or non-UTF-8 workflow arguments/environment.
pub fn rust_exec_step(
    name: &str,
    program: &OsStr,
    args: &[OsString],
    homes: &ToolHomes,
    catalog: &ToolCatalog,
) -> Result<Step, MiseError> {
    if name.trim().is_empty() || name.bytes().any(|byte| byte.is_ascii_control()) {
        return Err(invalid_step_input("name", name));
    }
    let request = PinnedToolExec::new(vec![PinnedTool::Rust], program, args.to_vec())?;
    shell_step(name, request.argv(catalog), homes.exec_env(catalog))
}

fn validate_mise_action_uses(uses: &str) -> Result<(), MiseError> {
    let valid = uses
        .strip_prefix(MISE_ACTION_PREFIX)
        .is_some_and(|sha| velnor_actions_contract::ids::is_lower_hex_len(sha, 40));
    if valid {
        Ok(())
    } else {
        Err(invalid_step_input("mise_action_uses", uses))
    }
}

fn shell_step(
    name: &str,
    argv: Vec<OsString>,
    env: Vec<(OsString, OsString)>,
) -> Result<Step, MiseError> {
    let run = argv
        .into_iter()
        .map(workflow_string)
        .collect::<Result<_, _>>()?;
    let mut env_map = BTreeMap::new();
    for (key, value) in env {
        if env_map
            .insert(workflow_string(key)?, workflow_string(value)?)
            .is_some()
        {
            return Err(invalid_step_input("env", "duplicate_key"));
        }
    }
    Ok(Step {
        name: name.to_owned(),
        condition: None,
        kind: StepKind::Shell { run, env: env_map },
    })
}

fn workflow_string(value: OsString) -> Result<String, MiseError> {
    value
        .into_string()
        .map_err(|_| invalid_step_input("workflow_value", "non_utf8"))
}

fn invalid_step_input(field: &str, value: &str) -> MiseError {
    MiseError::InvalidStepInput {
        field: field.to_owned(),
        value: value.to_owned(),
    }
}
