//! OpenTofu environment policy cases.

use std::ffi::OsString;
use velnor_actions_mise::command::is_reserved_env_key;
use velnor_actions_mise::{IsolatedCommand, MiseError};

#[test]
fn tofu_families_are_reserved_except_automation_pair() {
    for key in [
        "TF_DATA_DIR",
        "TF_CLI_CONFIG_FILE",
        "TF_PLUGIN_CACHE_DIR",
        "TF_VAR_secret",
        "TF_CLI_ARGS",
        "TF_CLI_ARGS_plan",
        "TF_TOKEN_app",
        "TF_WORKSPACE",
        "TF_LOG",
        "TF_LOG_PATH",
        "TF_REGISTRY_CLIENT_TIMEOUT",
        "TF_PLUGIN_CACHE_MAY_BREAK_DEPENDENCY_LOCK_FILE",
        "TOFU_FUTURE_KEY",
        "CHECKPOINT_DISABLE",
        "CHECKPOINT_TIMEOUT",
    ] {
        assert!(is_reserved_env_key(key), "{key} reserved");
    }
    for key in ["TF_IN_AUTOMATION", "TF_INPUT"] {
        assert!(!is_reserved_env_key(key), "{key} stays allowed");
    }
}

#[test]
fn tofu_reserved_keys_fail_loud_via_with_env_and_repo_task() -> Result<(), String> {
    for key in [
        "TF_DATA_DIR",
        "TF_CLI_CONFIG_FILE",
        "TF_VAR_hostile",
        "TF_CLI_ARGS_plan",
        "CHECKPOINT_DISABLE",
    ] {
        let pair = [(OsString::from(key), OsString::from("sentinel"))];
        let exec = IsolatedCommand::mise_exec(
            &["rust[profile=minimal,components=clippy,rustfmt]@1.98.1".to_owned()],
            &[OsString::from("cargo")],
        )
        .map_err(|err| err.to_string())?;
        assert!(
            matches!(
                exec.with_env(&pair),
                Err(MiseError::InvalidStepInput { field, value })
                    if field == key && value == "reserved_env_key"
            ),
            "{key} must fail loud via with_env"
        );
        let declared = vec![(OsString::from(key), OsString::from("sentinel"))];
        assert!(
            matches!(
                IsolatedCommand::repo_task("sh", Vec::new(), &declared),
                Err(MiseError::InvalidStepInput { field, value })
                    if field == key && value == "reserved_env_key"
            ),
            "{key} must fail loud via repo_task"
        );
    }
    Ok(())
}

#[test]
fn automation_pair_appends_as_allowed_extra() -> Result<(), String> {
    let command = IsolatedCommand::mise_exec(
        &["rust[profile=minimal,components=clippy,rustfmt]@1.98.1".to_owned()],
        &[OsString::from("cargo"), OsString::from("--version")],
    )
    .map_err(|err| err.to_string())?;
    let pair = vec![
        (OsString::from("TF_IN_AUTOMATION"), OsString::from("1")),
        (OsString::from("TF_INPUT"), OsString::from("0")),
    ];
    let extended = command.with_env(&pair).map_err(|err| err.to_string())?;
    let full = extended.full_env();
    assert_eq!(&full[full.len() - 2..], pair.as_slice());
    Ok(())
}
