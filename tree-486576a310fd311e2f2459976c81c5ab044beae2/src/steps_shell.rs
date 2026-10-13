use std::collections::BTreeMap;

use velnor_actions_contract::{Step, StepKind};

use crate::{RenderError, commands, steps};

/// Validated fixed-argv shell step, scrubbed and unset by construction.
///
/// Every `run:` step gets an empty-string credential overlay and removes
/// credentials through a shape-specific unset prelude. Callers pass the
/// base env; this constructor owns and adds the scrub overlay.
/// # Errors
pub fn shell_step(
    name: &str,
    argv: Vec<String>,
    env: BTreeMap<String, String>,
) -> Result<Step, RenderError> {
    shell_step_with_env_validation(name, argv, env, commands::validate_env)
}

/// Validated scrubbed shell step inside a generated composite action.
/// # Errors
pub(crate) fn composite_shell_step(
    name: &str,
    argv: Vec<String>,
    env: BTreeMap<String, String>,
) -> Result<Step, RenderError> {
    shell_step_with_env_validation(name, argv, env, commands::validate_composite_env)
}

fn shell_step_with_env_validation(
    name: &str,
    argv: Vec<String>,
    env: BTreeMap<String, String>,
    validate_env: fn(&BTreeMap<String, String>) -> Result<(), RenderError>,
) -> Result<Step, RenderError> {
    if name.trim().is_empty() {
        return Err(RenderError::BadCommand("empty_name".to_owned()));
    }
    crate::expressions::check_name_content(name)?;
    commands::validate_command_argv(&argv)?;
    validate_env(&env)?;
    crate::toolchain_env::reject_denied_step_keys(&env)?;
    steps::scan_for_private_subcommands(name)?;
    let run = if commands::is_inline_shell(&argv) {
        let mut scripted = argv;
        let preluded = crate::toolchain_env::with_credential_unset_script(&scripted[2]);
        scripted[2] = preluded;
        scripted
    } else {
        let mut run = crate::toolchain_env::with_env_unset_argv(&[]);
        run.extend(argv);
        run
    };
    let mut env_map = env;
    env_map.extend(crate::toolchain_env::credential_scrub());
    Ok(Step {
        name: name.to_owned(),
        id: None,
        role: None,
        condition: None,
        kind: StepKind::Shell { run, env: env_map },
    })
}

/// Validated fixed-argv shell step with ambient credentials intact.
///
/// Only repository-code-free operations that need network auth use this
/// exception: tool acquisition, Cargo Deny install/check, and publishing.
/// Validators prepared separately use [`shell_step`] for execution.
/// # Errors
pub fn ambient_shell_step(
    name: &str,
    argv: Vec<String>,
    env: BTreeMap<String, String>,
) -> Result<Step, RenderError> {
    if name.trim().is_empty() {
        return Err(RenderError::BadCommand("empty_name".to_owned()));
    }
    crate::expressions::check_name_content(name)?;
    commands::validate_command_argv(&argv)?;
    commands::validate_env(&env)?;
    steps::scan_for_private_subcommands(name)?;
    Ok(Step {
        name: name.to_owned(),
        id: None,
        role: None,
        condition: None,
        kind: StepKind::Shell { run: argv, env },
    })
}
