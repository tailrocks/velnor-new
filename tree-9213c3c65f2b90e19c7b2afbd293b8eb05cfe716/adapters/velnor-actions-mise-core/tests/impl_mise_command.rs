//! Fixed subprocess wrapper cases.
use std::ffi::{OsStr, OsString};
use velnor_actions_mise_core::command::{
    SPAWN_CANCELLED_MESSAGE, SPAWN_TIMEOUT_MESSAGE_PREFIX, is_cancel_or_timeout,
};
use velnor_actions_mise_core::{
    ISOLATION_ENV, IsolatedCommand, MISE_GLOBAL_FLAGS, MiseError, ProcessOutput,
    TOOL_COMMAND_SEPARATOR,
};

fn specs() -> Vec<String> {
    vec!["rust@1.98.1".to_owned()]
}

fn payload() -> Vec<OsString> {
    vec![
        OsString::from("cargo"),
        OsString::from("metadata"),
        OsString::from("--format-version"),
    ]
}

#[test]
fn isolation_env_carries_the_quartet() {
    assert_eq!(
        ISOLATION_ENV,
        [
            ("MISE_NO_CONFIG", "1"),
            ("MISE_NO_ENV", "1"),
            ("MISE_NO_HOOKS", "1"),
            ("MISE_LOCKFILE", "0"),
        ]
    );
    let overlay = IsolatedCommand::env_overlay();
    let expected: Vec<(OsString, OsString)> = ISOLATION_ENV
        .iter()
        .map(|(key, value)| (OsString::from(key), OsString::from(value)))
        .collect();
    assert_eq!(overlay, expected);
}

#[test]
fn mise_argv_places_globals_before_subcommand() -> Result<(), String> {
    let command =
        IsolatedCommand::mise_exec(&specs(), &payload()).map_err(|err| err.to_string())?;
    assert_eq!(
        command.argv(),
        vec![
            OsString::from("mise"),
            OsString::from("--no-config"),
            OsString::from("--no-env"),
            OsString::from("--no-hooks"),
            OsString::from("exec"),
            OsString::from("rust@1.98.1"),
            OsString::from("--"),
            OsString::from("cargo"),
            OsString::from("metadata"),
            OsString::from("--format-version"),
        ]
    );
    assert_eq!(command.program(), "mise");
    assert_eq!(MISE_GLOBAL_FLAGS, ["--no-config", "--no-env", "--no-hooks"]);
    assert_eq!(TOOL_COMMAND_SEPARATOR, "--");
    Ok(())
}

#[test]
fn separator_splits_specs_from_payload() -> Result<(), String> {
    let command =
        IsolatedCommand::mise_exec(&specs(), &payload()).map_err(|err| err.to_string())?;
    let argv = command.argv();
    let separators = argv.iter().filter(|arg| arg.as_os_str() == "--").count();
    assert_eq!(separators, 1);
    let split = argv.iter().position(|arg| arg == "--").unwrap_or(0);
    assert_eq!(
        argv[..split].last().map(OsString::as_os_str),
        Some(OsStr::new("rust@1.98.1"))
    );
    assert_eq!(argv[split + 1], OsString::from("cargo"));
    Ok(())
}

#[test]
fn args_pass_through_byte_exact() -> Result<(), String> {
    let tricky = vec![
        OsString::from("--manifest-path"),
        OsString::from("/tmp/x y/Cargo.toml"),
        OsString::from("--flag-after-separator"),
        OsString::from("key=value with spaces"),
        OsString::from("héllo-🦀"),
        OsString::from(""),
    ];
    let command = IsolatedCommand::mise_exec(&specs(), &tricky).map_err(|err| err.to_string())?;
    let argv = command.argv();
    let split = argv.iter().position(|arg| arg == "--").unwrap_or(0);
    assert_eq!(&argv[split + 1..], tricky.as_slice());
    Ok(())
}

#[test]
fn no_shell_interprets_any_argument() -> Result<(), String> {
    let hostile = [
        OsString::from("cargo"),
        OsString::from("metadata; rm -rf /"),
        OsString::from("$(whoami)"),
    ];
    let command = IsolatedCommand::mise_exec(&specs(), &hostile).map_err(|err| err.to_string())?;
    let argv = command.argv();
    assert_eq!(
        argv.first().map(OsString::as_os_str),
        Some(OsStr::new("mise"))
    );
    for shell in ["sh", "bash", "dash", "-c", "/bin/sh"] {
        assert!(
            !argv.iter().any(|arg| arg == shell),
            "shell token must never appear: {shell}"
        );
    }
    assert!(
        argv.iter().any(|arg| arg == "metadata; rm -rf /"),
        "metacharacters stay inside one argument"
    );
    Ok(())
}

#[test]
fn empty_payload_is_rejected() {
    assert!(matches!(
        IsolatedCommand::mise_exec(&specs(), &[]),
        Err(velnor_actions_mise_core::MiseError::EmptyCommand { .. })
    ));
}

#[test]
fn with_cwd_records_working_directory() -> Result<(), String> {
    let command =
        IsolatedCommand::mise_exec(&specs(), &payload()).map_err(|err| err.to_string())?;
    assert_eq!(command.cwd(), None);
    let custom = std::env::temp_dir();
    let moved = command.with_cwd(custom.clone());
    assert_eq!(moved.cwd(), Some(&custom));
    Ok(())
}

#[test]
fn process_output_reports_typed_exit() {
    let ok = ProcessOutput {
        stdout: b"{}".to_vec(),
        stderr: Vec::new(),
        code: Some(0),
        signal: None,
        success: true,
    };
    assert!(ok.require_success("mise").is_ok());
    assert_eq!(ok.stdout_text("mise"), Ok("{}".to_owned()));

    let failed = ProcessOutput {
        stdout: Vec::new(),
        stderr: b"boom".to_vec(),
        code: Some(1),
        signal: None,
        success: false,
    };
    assert!(matches!(
        failed.require_success("mise"),
        Err(velnor_actions_mise_core::MiseError::NonZeroExit { code: Some(1), .. })
    ));

    let binary = ProcessOutput {
        stdout: vec![0xff, 0xfe],
        stderr: Vec::new(),
        code: Some(0),
        signal: None,
        success: true,
    };
    assert!(matches!(
        binary.stdout_text("mise"),
        Err(velnor_actions_mise_core::MiseError::InvalidUtf8 { .. })
    ));
}

#[test]
fn cancel_or_timeout_classifier_separates_abortions_from_outcomes() {
    assert_eq!(SPAWN_CANCELLED_MESSAGE, "cancelled");
    assert_eq!(SPAWN_TIMEOUT_MESSAGE_PREFIX, "timeout_after_secs:");
    for message in [
        "cancelled",
        "timeout_after_secs:1",
        "timeout_after_secs:600",
        "cancelled;cleanup_failed:kill_group:permission denied",
        "timeout_after_absolute_deadline;cleanup_failed:reap_child:still running",
        "timeout_after_secs:30;cleanup_failed:kill_group:permission denied",
    ] {
        let error = MiseError::SpawnFailed {
            program: "sh".to_owned(),
            message: message.to_owned(),
        };
        assert!(is_cancel_or_timeout(&error), "{message} must classify");
    }
    for message in [
        "cancelled_extra;cleanup_failed:permission denied",
        "timeout_after_absolute_deadline_extra;cleanup_failed:still running",
        "cancelled;cleanup_failed:",
    ] {
        let error = MiseError::SpawnFailed {
            program: "sh".to_owned(),
            message: message.to_owned(),
        };
        assert!(!is_cancel_or_timeout(&error), "{message} must not classify");
    }
    for error in [
        MiseError::SpawnFailed {
            program: "sh".to_owned(),
            message: "stdout_limit_exceeded:1024".to_owned(),
        },
        MiseError::SpawnFailed {
            program: "no-such-program".to_owned(),
            message: "No such file or directory (os error 2)".to_owned(),
        },
        MiseError::NonZeroExit {
            program: "sh".to_owned(),
            code: Some(1),
            stderr: String::new(),
        },
        MiseError::CacheNotEligible {
            task: "clippy".to_owned(),
            reason: "forced_uncached".to_owned(),
        },
    ] {
        assert!(!is_cancel_or_timeout(&error), "{error} must not classify");
    }
}

#[test]
fn custom_task_run_shape_is_plain_mise_run() {
    use velnor_actions_mise_core::custom_run::custom_task_run_argv;
    let argv = custom_task_run_argv("audit").expect("custom argv");
    assert_eq!(argv.join(" "), "mise run audit");
    let namespaced = custom_task_run_argv("lint:strict").expect("namespaced task");
    assert_eq!(namespaced.join(" "), "mise run lint:strict");
    for bad in [
        "",
        "  ",
        "two words",
        "a/b",
        "${{secrets.x}}",
        "a;true",
        "a`id`",
        "$(id)",
        "a'b",
        "a\"b",
        "--help",
        "-x",
        ".hidden",
        "./audit",
        ":leading",
    ] {
        assert!(
            custom_task_run_argv(bad).is_err(),
            "{bad:?} must fail closed"
        );
    }
}
