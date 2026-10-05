//! Typed validator commands and their repository-source closure.

use std::collections::BTreeSet;

use velnor_actions_contract::ValidatorKind;

use crate::{RenderError, commands, guard};

/// Display name of the install-only Python test-runtime step.
pub const PYTHON_SOURCE_PREPARE_NAME: &str = "Prepare Python source tests";
/// Display name of the scrubbed Python source-suite step.
pub const PYTHON_SOURCE_RUN_NAME: &str = "Run Python source tests";

/// Role of one file in a repository source-unit input closure.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ValidatorSourceInputKind {
    /// Runtime module imported or called by the tests.
    Module,
    /// Test module discovered by unittest.
    Test,
    /// Static fixture read by a test or runtime module.
    Fixture,
    /// Repository configuration that fixes suite behavior.
    Configuration,
    /// Catalog source that fixes an installed tool version.
    ToolchainPin,
}

/// One relative file in a source-unit closure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidatorSourceInput {
    /// Repository-relative file path.
    pub path: String,
    /// Why this file belongs to the closure.
    pub kind: ValidatorSourceInputKind,
}

/// One fixed unittest suite and all files it reads or imports.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidatorSourceUnit {
    /// Stable suite identity.
    pub id: String,
    /// Directory passed to unittest discovery.
    pub discovery_root: String,
    /// Optional unittest filename pattern.
    pub pattern: Option<String>,
    /// Test, module, and fixture paths in this suite's closure.
    pub inputs: Vec<ValidatorSourceInput>,
}

/// One repository validator command plus optional preparation and source closure.
#[derive(Debug, Clone)]
pub struct ValidatorCommand {
    /// Repository validator owning this step's job.
    pub validator: ValidatorKind,
    /// Step display name.
    pub name: String,
    /// Fixed argument vector.
    pub argv: Vec<String>,
    /// Optional ambient-auth, install-only command that prepares this validator.
    pub prepare_argv: Vec<String>,
    /// Source units run by `PythonSourceTests`.
    pub source_units: Vec<ValidatorSourceUnit>,
    /// Shared Python and Mise pin/configuration inputs.
    pub tool_inputs: Vec<ValidatorSourceInput>,
}

/// Validate command vectors and typed closure before rendering.
pub(crate) fn validate(command: &ValidatorCommand) -> Result<(), RenderError> {
    if command.validator == ValidatorKind::Actionlint {
        return Err(RenderError::BadCommand("actionlint_not_support".to_owned()));
    }
    if command.name.trim().is_empty() {
        return Err(RenderError::BadCommand("empty_validator_name".to_owned()));
    }
    commands::validate_command_argv(&command.argv)?;
    if command.validator == ValidatorKind::PythonSourceTests {
        if command.prepare_argv.is_empty() {
            return Err(RenderError::BadCommand(
                "python_source_tests_without_prepare".to_owned(),
            ));
        }
        validate_source_closure(command)?;
    } else if !command.prepare_argv.is_empty()
        || !command.source_units.is_empty()
        || !command.tool_inputs.is_empty()
    {
        return Err(RenderError::BadCommand(
            "validator_python_source_fields_not_supported".to_owned(),
        ));
    }
    if !command.prepare_argv.is_empty() {
        commands::validate_command_argv(&command.prepare_argv)?;
    }
    Ok(())
}

/// Validate suite identities, discovery paths, input roles, and uniqueness.
fn validate_source_closure(command: &ValidatorCommand) -> Result<(), RenderError> {
    if command.source_units.is_empty() || command.tool_inputs.is_empty() {
        return Err(RenderError::BadCommand(
            "python_source_closure_empty".to_owned(),
        ));
    }
    let mut unit_ids = BTreeSet::<String>::new();
    for unit in &command.source_units {
        validate_unit(unit, &mut unit_ids)?;
    }
    let mut tool_paths = BTreeSet::new();
    let mut has_configuration = false;
    let mut has_toolchain_pin = false;
    for input in &command.tool_inputs {
        validate_input(input)?;
        match input.kind {
            ValidatorSourceInputKind::Configuration => has_configuration = true,
            ValidatorSourceInputKind::ToolchainPin => has_toolchain_pin = true,
            _ => {
                return Err(RenderError::BadCommand(format!(
                    "bad_python_tool_input_kind:{}",
                    input.path
                )));
            }
        }
        if !tool_paths.insert(input.path.as_str()) {
            return Err(RenderError::BadCommand(format!(
                "duplicate_python_tool_input:{}",
                input.path
            )));
        }
    }
    if !has_configuration || !has_toolchain_pin {
        return Err(RenderError::BadCommand(
            "python_tool_inputs_missing_config_or_pin".to_owned(),
        ));
    }
    Ok(())
}

/// Validate one suite and ensure it binds a test module below its root.
fn validate_unit(
    unit: &ValidatorSourceUnit,
    unit_ids: &mut BTreeSet<String>,
) -> Result<(), RenderError> {
    if unit.id.is_empty()
        || !unit
            .id
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
        || !unit_ids.insert(unit.id.clone())
    {
        return Err(RenderError::BadCommand(format!(
            "bad_python_source_unit_id:{}",
            unit.id
        )));
    }
    guard::validate_tree_path(&unit.discovery_root)?;
    if let Some(pattern) = &unit.pattern {
        guard::validate_tree_path(pattern)?;
        if pattern.contains('/') {
            return Err(RenderError::BadCommand(format!(
                "python_test_pattern_has_path:{pattern}"
            )));
        }
    }
    if unit.inputs.is_empty() {
        return Err(RenderError::BadCommand(format!(
            "python_source_unit_without_inputs:{}",
            unit.id
        )));
    }
    let prefix = format!("{}/", unit.discovery_root);
    let mut paths = BTreeSet::new();
    let mut has_test = false;
    for input in &unit.inputs {
        validate_input(input)?;
        if input.kind == ValidatorSourceInputKind::Test && !input.path.starts_with(&prefix) {
            return Err(RenderError::BadCommand(format!(
                "python_test_input_outside_unit:{}:{}",
                unit.id, input.path
            )));
        }
        if !paths.insert(input.path.as_str()) {
            return Err(RenderError::BadCommand(format!(
                "duplicate_python_source_input:{}:{}",
                unit.id, input.path
            )));
        }
        has_test |= input.kind == ValidatorSourceInputKind::Test;
    }
    if !has_test {
        return Err(RenderError::BadCommand(format!(
            "python_source_unit_without_test:{}",
            unit.id
        )));
    }
    Ok(())
}

/// Validate a repository-relative input file path.
fn validate_input(input: &ValidatorSourceInput) -> Result<(), RenderError> {
    guard::validate_tree_path(&input.path)?;
    Ok(())
}

/// Add exact source-closure existence checks before the fixed Python command.
pub(crate) fn execution_argv(command: &ValidatorCommand) -> Result<Vec<String>, RenderError> {
    if command.validator != ValidatorKind::PythonSourceTests {
        return Ok(command.argv.clone());
    }
    let [shell, flag, script] = command.argv.as_slice() else {
        return Err(RenderError::BadCommand(
            "python_source_tests_require_inline_shell".to_owned(),
        ));
    };
    if shell != "sh" || flag != "-c" {
        return Err(RenderError::BadCommand(
            "python_source_tests_require_sh_c".to_owned(),
        ));
    }
    let mut directories = BTreeSet::new();
    let mut files = BTreeSet::new();
    for input in &command.tool_inputs {
        files.insert(input.path.as_str());
    }
    for unit in &command.source_units {
        directories.insert(unit.discovery_root.as_str());
        for input in &unit.inputs {
            files.insert(input.path.as_str());
        }
    }
    let mut checks = Vec::with_capacity(directories.len() + files.len());
    checks.extend(
        directories
            .into_iter()
            .map(|path| format!("test -d {}", commands::quote_run_arg(path))),
    );
    checks.extend(
        files
            .into_iter()
            .map(|path| format!("test -f {}", commands::quote_run_arg(path))),
    );
    let mut argv = command.argv.clone();
    argv[2] = format!("set -eu; {}; {script}", checks.join("; "));
    commands::validate_command_argv(&argv)?;
    Ok(argv)
}
