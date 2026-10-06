//! Tofu spawn-constructor cases: pinned argv, baked isolation env,
//! input rejection, and exit/cancel classification.
use std::ffi::{OsStr, OsString};
use std::time::Duration;
use velnor_actions_mise::command::{
    CancelHandle, IsolatedCommand, is_cancel_or_timeout, is_reserved_env_key,
};
use velnor_actions_mise::{
    MiseError, PinnedTool, PinnedToolExec, ProcessOutput, TF_CLI_CONFIG_FILE_ENV, TF_DATA_DIR_ENV,
    TF_IN_AUTOMATION_ENV, TF_IN_AUTOMATION_ON, TF_INPUT_ENV, TF_INPUT_OFF, TF_PLUGIN_CACHE_DIR_ENV,
    ToolCatalog,
};

/// Fixed fmt payload mirroring the tofu adapter's fixed shape for a
/// subdir root (`-chdir` first, recursive check, no file operands).
fn fmt_payload() -> Vec<OsString> {
    [
        "tofu",
        "-chdir",
        "stacks/vpc",
        "fmt",
        "-check",
        "-recursive",
        "-no-color",
    ]
    .iter()
    .map(OsString::from)
    .collect()
}

fn tofu_command() -> Result<IsolatedCommand, String> {
    IsolatedCommand::tofu_exec(
        &["opentofu@1.13.1".to_owned()],
        &fmt_payload(),
        "/velnor/tofu-data",
        "/velnor/tofu-cli.hcl",
        "/velnor/tofu-cache",
    )
    .map_err(|err| err.to_string())
}

#[test]
fn tofu_key_consts_are_exact() {
    assert_eq!(TF_IN_AUTOMATION_ENV, "TF_IN_AUTOMATION");
    assert_eq!(TF_IN_AUTOMATION_ON, "1");
    assert_eq!(TF_INPUT_ENV, "TF_INPUT");
    assert_eq!(TF_INPUT_OFF, "0");
    assert_eq!(TF_DATA_DIR_ENV, "TF_DATA_DIR");
    assert_eq!(TF_CLI_CONFIG_FILE_ENV, "TF_CLI_CONFIG_FILE");
    assert_eq!(TF_PLUGIN_CACHE_DIR_ENV, "TF_PLUGIN_CACHE_DIR");
}

#[test]
fn tofu_exec_wraps_pinned_opentofu_payload() -> Result<(), String> {
    let catalog = ToolCatalog::pinned();
    let exec = PinnedToolExec::new(
        vec![PinnedTool::Opentofu],
        OsStr::new("tofu"),
        fmt_payload()[1..].to_vec(),
    )
    .map_err(|err| err.to_string())?;
    let specs = catalog.tool_specs(exec.tools());
    assert_eq!(specs, ["opentofu@1.13.1".to_owned()]);
    let command = IsolatedCommand::tofu_exec(
        &specs,
        &exec.payload(),
        "/velnor/tofu-data",
        "/velnor/tofu-cli.hcl",
        "/velnor/tofu-cache",
    )
    .map_err(|err| err.to_string())?;
    let argv: Vec<String> = command
        .argv()
        .iter()
        .map(|arg| arg.to_string_lossy().into_owned())
        .collect();
    assert_eq!(
        argv,
        [
            "mise",
            "--no-config",
            "--no-env",
            "--no-hooks",
            "exec",
            "opentofu@1.13.1",
            "--",
            "tofu",
            "-chdir",
            "stacks/vpc",
            "fmt",
            "-check",
            "-recursive",
            "-no-color",
        ]
    );
    Ok(())
}

#[test]
fn tofu_exec_bakes_exact_isolation_env() -> Result<(), String> {
    let full = tofu_command()?.full_env();
    let texts: Vec<(String, String)> = full
        .iter()
        .map(|(key, value)| {
            (
                key.to_string_lossy().into_owned(),
                value.to_string_lossy().into_owned(),
            )
        })
        .collect();
    assert_eq!(
        texts,
        [
            ("MISE_NO_CONFIG".to_owned(), "1".to_owned()),
            ("MISE_NO_ENV".to_owned(), "1".to_owned()),
            ("MISE_NO_HOOKS".to_owned(), "1".to_owned()),
            ("MISE_LOCKFILE".to_owned(), "0".to_owned()),
            ("MISE_AUTO_INSTALL".to_owned(), "false".to_owned()),
            ("MISE_EXEC_AUTO_INSTALL".to_owned(), "false".to_owned()),
            ("TF_IN_AUTOMATION".to_owned(), "1".to_owned()),
            ("TF_INPUT".to_owned(), "0".to_owned()),
            ("TF_DATA_DIR".to_owned(), "/velnor/tofu-data".to_owned()),
            (
                "TF_CLI_CONFIG_FILE".to_owned(),
                "/velnor/tofu-cli.hcl".to_owned()
            ),
            (
                "TF_PLUGIN_CACHE_DIR".to_owned(),
                "/velnor/tofu-cache".to_owned()
            ),
        ]
    );
    Ok(())
}

#[test]
fn tofu_exec_rejects_empty_inputs() {
    let specs = ["opentofu@1.13.1".to_owned()];
    assert!(matches!(
        IsolatedCommand::tofu_exec(&specs, &[], "/data", "/cli.hcl", "/cache"),
        Err(MiseError::EmptyCommand { .. })
    ));
    assert!(matches!(
        IsolatedCommand::tofu_exec(&specs, &fmt_payload(), "", "/cli.hcl", "/cache"),
        Err(MiseError::InvalidStepInput { field, .. }) if field == "tf_data_dir"
    ));
    assert!(matches!(
        IsolatedCommand::tofu_exec(&specs, &fmt_payload(), "/data", "", "/cache"),
        Err(MiseError::InvalidStepInput { field, .. }) if field == "tf_cli_config_file"
    ));
    assert!(matches!(
        IsolatedCommand::tofu_exec(&specs, &fmt_payload(), "/data", "/cli.hcl", ""),
        Err(MiseError::InvalidStepInput { field, .. }) if field == "tf_plugin_cache_dir"
    ));
}

#[test]
fn tofu_exec_selects_verify_policy_and_disables_install() -> Result<(), String> {
    let command = tofu_command()?;
    let debug = format!("{command:?}");
    assert!(debug.contains("Verify"), "tofu exec stays Verify: {debug}");
    assert!(command.disables_auto_install());
    assert_eq!(command.cwd(), None);
    Ok(())
}

#[test]
fn tofu_exec_stages_cwd() -> Result<(), String> {
    let staging = std::env::temp_dir().join("velnor-tofu-staging-probe");
    let moved = tofu_command()?.with_cwd(staging.clone());
    assert_eq!(moved.cwd(), Some(&staging));
    Ok(())
}

#[test]
fn tofu_baked_keys_stay_reserved_as_extras() -> Result<(), String> {
    for key in ["TF_DATA_DIR", "TF_CLI_CONFIG_FILE"] {
        assert!(is_reserved_env_key(key), "{key} reserved");
        let hostile = [(OsString::from(key), OsString::from("/evil"))];
        assert!(
            matches!(
                tofu_command()?.with_env(&hostile),
                Err(MiseError::InvalidStepInput { value, .. }) if value == "reserved_env_key"
            ),
            "{key} override must fail loud, never silent-drop"
        );
    }
    Ok(())
}

#[test]
fn tofu_precancelled_run_fails_typed_without_spawning() -> Result<(), String> {
    let cancel = CancelHandle::new();
    cancel.cancel();
    let err = tofu_command()?
        .run_cancellable(1024, Duration::from_secs(60), &cancel)
        .expect_err("precancelled tofu run must fail without spawning");
    assert!(is_cancel_or_timeout(&err), "cancel must classify: {err}");
    assert!(
        matches!(&err, MiseError::SpawnFailed { message, .. } if message == "cancelled"),
        "got {err}"
    );
    Ok(())
}

/// Tofu failure modes against the shared classifier: cancel/timeout
/// propagate, nonzero exits (including fmt `-check` exit 3) and
/// spawn failures stay data/outcomes, never abortions.
#[test]
fn tofu_exit_modes_separate_outcomes_from_abortions() {
    for message in ["cancelled", "timeout_after_secs:600"] {
        let error = MiseError::SpawnFailed {
            program: "mise".to_owned(),
            message: message.to_owned(),
        };
        assert!(is_cancel_or_timeout(&error), "{message} must classify");
    }
    let fmt_check_failed = ProcessOutput {
        stdout: Vec::new(),
        stderr: b"stacks/vpc/main.tf".to_vec(),
        code: Some(3),
        signal: None,
        success: false,
    };
    assert!(matches!(
        fmt_check_failed.require_success("mise"),
        Err(MiseError::NonZeroExit { code: Some(3), .. })
    ));
    for error in [
        MiseError::NonZeroExit {
            program: "mise".to_owned(),
            code: Some(3),
            stderr: "stacks/vpc/main.tf".to_owned(),
        },
        MiseError::NonZeroExit {
            program: "mise".to_owned(),
            code: Some(1),
            stderr: String::new(),
        },
        MiseError::SpawnFailed {
            program: "mise".to_owned(),
            message: "No such file or directory (os error 2)".to_owned(),
        },
    ] {
        assert!(
            !is_cancel_or_timeout(&error),
            "{error} must stay an outcome"
        );
    }
}
