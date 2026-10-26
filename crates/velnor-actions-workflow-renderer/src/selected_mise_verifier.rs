// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_actions_contract::config::{
    is_exact_semver, jactionlint_verifier::jactionlint_verifier_script as contract_verifier_script,
    selected_mise::BIND_SELECTED_MISE_FUNCTION,
};

use crate::{RenderError, VERIFY_PATH, commands};

const VALIDATION_MARKER: &str = "VELNOR_SELECTED_MISE_BINDER";
const VERSION_END: &str = "\"; case \"${RUNNER_OS-}-${RUNNER_ARCH-}\" in ";

pub(crate) fn script_for_version(version: &str) -> String {
    contract_verifier_script(version, VERIFY_PATH)
}

pub(crate) fn validate_argv(argv: &[String]) -> Result<(), RenderError> {
    validate_jactionlint_argv(argv)
}

pub(crate) fn validate_role_argv(argv: &[String]) -> Result<(), RenderError> {
    let Some(script) = argv.get(2) else {
        return Err(invalid_binding("verifier_shell_shape_mismatch"));
    };
    let prelude = format!("{} ", crate::toolchain_env::credential_unset_prelude());
    let script = script
        .strip_prefix(&prelude)
        .ok_or_else(|| invalid_binding("verifier_credential_prelude_missing"))?;
    validate_jactionlint_argv_with_script(argv, script)
}

fn validate_jactionlint_argv(argv: &[String]) -> Result<(), RenderError> {
    if argv.len() != 3 || argv[0] != "sh" || argv[1] != "-c" {
        return Err(invalid_binding("verifier_shell_shape_mismatch"));
    }
    validate_jactionlint_argv_with_script(argv, &argv[2])
}

fn validate_jactionlint_argv_with_script(argv: &[String], script: &str) -> Result<(), RenderError> {
    if argv.len() != 3 || argv[0] != "sh" || argv[1] != "-c" {
        return Err(invalid_binding("verifier_shell_shape_mismatch"));
    }
    let version_prefix = format!("set -eu; PATH=\"{VERIFY_PATH}\"; export PATH; v=\"");
    let version = script
        .strip_prefix(&version_prefix)
        .and_then(|tail| tail.split_once(VERSION_END).map(|(version, _)| version))
        .ok_or_else(|| invalid_binding("jactionlint_verifier_prefix_invalid"))?;
    if !is_exact_semver(version) && version != "${VELNOR_TOOL_JACTIONLINT}" {
        return Err(invalid_binding("jactionlint_verifier_version_invalid"));
    }
    let canonical_script = script_for_version(version);
    if script != canonical_script {
        return Err(invalid_binding("jactionlint_verifier_script_mismatch"));
    }
    let (before, after) = script
        .split_once(BIND_SELECTED_MISE_FUNCTION)
        .ok_or_else(|| invalid_binding("mise_binder_missing"))?;

    let mut validation_argv = argv.to_vec();
    validation_argv[2] = format!("{before}{VALIDATION_MARKER}{after}");
    commands::validate_command_argv(&validation_argv)
}

fn invalid_binding(code: &str) -> RenderError {
    RenderError::BadCommand(code.to_owned())
}
