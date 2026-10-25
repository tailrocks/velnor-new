use super::direct_tool_policy_violation;

#[test]
fn process_methods_require_the_process_command_capability() {
    for method in ["spawn", "status", "output"] {
        let command = format!(
            r#"fn launch() {{ std::process::Command::new("cargo").{method}().unwrap(); }}"#
        );
        assert_eq!(
            direct_tool_policy_violation(&command),
            Some("process::Command".to_owned())
        );

        let grouped = format!(
            r#"
                use std::process::{{Command as Process, ExitCode}};
                fn launch() {{ Process::new("cargo").{method}().unwrap(); }}
            "#
        );
        assert_eq!(
            direct_tool_policy_violation(&grouped),
            Some("Process::new".to_owned())
        );

        let qualified =
            format!("fn launch(command: std::process::Command) {{ command.{method}(); }}");
        assert_eq!(
            direct_tool_policy_violation(&qualified),
            Some("process::Command".to_owned())
        );

        let renamed = format!(
            "use std::process::Command as Process; fn launch(command: Process) {{ command.{method}(); }}"
        );
        assert_eq!(
            direct_tool_policy_violation(&renamed),
            Some("process::Command".to_owned())
        );

        let unrelated = format!("fn inspect(proof: CacheRestoreProof) {{ proof.{method}(); }}");
        assert_eq!(direct_tool_policy_violation(&unrelated), None);
    }

    assert_eq!(
        direct_tool_policy_violation("fn process_id() { let _pid = std::process::id(); }"),
        None
    );
    assert_eq!(
        direct_tool_policy_violation(
            "fn exit() -> std::process::ExitCode { std::process::ExitCode::SUCCESS }"
        ),
        None
    );
}
