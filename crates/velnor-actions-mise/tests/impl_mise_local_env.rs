//! Local Mise spawn boundary cases.
use std::ffi::OsString;
use velnor_actions_mise::{GitRequest, IsolatedCommand, MiseError};

#[test]
fn local_mise_data_dir_is_absolute_and_expression_free() -> Result<(), String> {
    let pair = [(
        OsString::from("MISE_DATA_DIR"),
        OsString::from("/velnor/mise"),
    )];
    let exec = IsolatedCommand::mise_exec(&["rust@1.98.1".to_owned()], &[OsString::from("cargo")])
        .map_err(|err| err.to_string())?
        .with_env(&pair)
        .map_err(|err| err.to_string())?;
    assert!(
        exec.full_env()
            .iter()
            .any(|(key, value)| { key == "MISE_DATA_DIR" && value == "/velnor/mise" })
    );
    let expression = [(
        OsString::from("MISE_DATA_DIR"),
        OsString::from("${{ runner.temp }}/velnor/mise"),
    )];
    assert!(matches!(
        IsolatedCommand::mise_exec(&["rust@1.98.1".to_owned()], &[OsString::from("cargo")])
            .map_err(|err| err.to_string())
            .and_then(|command| command.with_env(&expression).map_err(|err| err.to_string())),
        Err(_)
    ));
    assert!(matches!(
        IsolatedCommand::repo_task("sh", Vec::new(), &pair),
        Err(MiseError::InvalidStepInput { .. })
    ));
    assert!(matches!(
        GitRequest::rev_parse(Vec::new()).command().with_env(&pair),
        Err(MiseError::InvalidStepInput { .. })
    ));
    let relative_home = [(OsString::from("MISE_CARGO_HOME"), OsString::from("cargo"))];
    assert!(matches!(
        IsolatedCommand::mise_exec(&["rust@1.98.1".to_owned()], &[OsString::from("cargo")])
            .map_err(|err| err.to_string())
            .and_then(|command| command
                .with_env(&relative_home)
                .map_err(|err| err.to_string())),
        Err(_)
    ));
    Ok(())
}

#[test]
fn unresolved_workflow_expression_is_rejected_by_local_spawn_boundary() -> Result<(), String> {
    let pair = [(
        OsString::from("CUSTOM_INPUT"),
        OsString::from("${{ github.ref }}"),
    )];
    let command =
        IsolatedCommand::mise_exec(&["rust@1.98.1".to_owned()], &[OsString::from("cargo")])
            .map_err(|err| err.to_string())?;
    assert!(matches!(
        command.with_env(&pair),
        Err(MiseError::InvalidStepInput { field, value })
            if field == "CUSTOM_INPUT" && value == "unresolved_workflow_expression"
    ));
    assert!(matches!(
        IsolatedCommand::repo_task("sh", Vec::new(), &pair),
        Err(MiseError::InvalidStepInput { field, value })
            if field == "CUSTOM_INPUT" && value == "unresolved_workflow_expression"
    ));
    Ok(())
}
