use std::collections::BTreeMap;

use velnor_actions_contract::{Step, StepKind};

use crate::{RenderError, commands, steps};

pub fn shell_step(
    name: &str,
    argv: Vec<String>,
    env: BTreeMap<String, String>,
) -> Result<Step, RenderError> {
    shell_step_with_env_validation(
        name,
        argv,
        env,
        commands::validate_env,
        commands::validate_command_argv,
    )
}

pub fn jactionlint_verifier_step(
    name: &str,
    argv: Vec<String>,
    env: BTreeMap<String, String>,
) -> Result<Step, RenderError> {
    let mut step = shell_step_with_env_validation(
        name,
        argv,
        env,
        commands::validate_env,
        crate::selected_mise_verifier::validate_argv,
    )?;
    step.role = Some(velnor_actions_contract::StepRole::VerifyJactionlint);
    Ok(step)
}

pub(crate) fn composite_shell_step(
    name: &str,
    argv: Vec<String>,
    env: BTreeMap<String, String>,
) -> Result<Step, RenderError> {
    shell_step_with_env_validation(
        name,
        argv,
        env,
        commands::validate_composite_env,
        commands::validate_command_argv,
    )
}

pub(crate) fn action_owned_mbx_shell_step(
    name: &str,
    argv: Vec<String>,
    env: BTreeMap<String, String>,
) -> Result<Step, RenderError> {
    shell_step_with_env_validation(
        name,
        argv,
        env,
        commands::validate_env,
        crate::action_mbx_shell::validate_argv,
    )
}

fn shell_step_with_env_validation(
    name: &str,
    argv: Vec<String>,
    env: BTreeMap<String, String>,
    validate_env: fn(&BTreeMap<String, String>) -> Result<(), RenderError>,
    validate_argv: fn(&[String]) -> Result<(), RenderError>,
) -> Result<Step, RenderError> {
    if name.trim().is_empty() {
        return Err(RenderError::BadCommand("empty_name".to_owned()));
    }
    crate::expressions::check_name_content(name)?;
    validate_argv(&argv)?;
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
        parallel: false,
        name: name.to_owned(),
        id: None,
        role: None,
        condition: None,
        kind: StepKind::Shell { run, env: env_map },
    })
}

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
        parallel: false,
        name: name.to_owned(),
        id: None,
        role: None,
        condition: None,
        kind: StepKind::Shell { run: argv, env },
    })
}
