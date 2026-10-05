//! Inline-shell command-substitution rejection cases.
use velnor_actions_workflow_renderer::{RenderError, join_argv_for_run, validate_command_argv};

fn argv(items: &[&str]) -> Vec<String> {
    items.iter().map(ToString::to_string).collect()
}

#[test]
fn generic_inline_shell_rejects_substitution_even_when_quoted() {
    for script in [
        "echo $(evil)",
        "echo \"$(sleep 1&echo done)\"",
        "echo `evil`",
        "echo \"`sleep 1&echo done`\"",
    ] {
        let err = validate_command_argv(&argv(&["bash", "-c", script]))
            .expect_err("command substitution must be rejected");
        assert!(
            err.to_string().contains("command_substitution"),
            "wrong rejection for {script}: {err}"
        );
    }
}

#[test]
fn env_separator_assignments_do_not_hide_shell_operators() {
    for assignment in ["FOO=bar", "FOO-BAR=value", "=value"] {
        let err = validate_command_argv(&argv(&[
            "env",
            "--",
            assignment,
            "bash",
            "-c",
            "sleep 1&echo done",
        ]))
        .expect_err("env assignment after -- must not hide a shell launcher");
        assert!(err.to_string().contains("background_shell"), "{err}");
    }
}

#[test]
fn env_separator_ignore_environment_marker_does_not_hide_shell_operators() {
    let err = validate_command_argv(&argv(&[
        "env",
        "--",
        "-",
        "FOO=bar",
        "bash",
        "-c",
        "sleep 1&echo done",
    ]))
    .expect_err("env ignore marker after -- must not hide a shell launcher");
    assert!(err.to_string().contains("background_shell"), "{err}");
}

#[test]
fn env_separator_assignment_keeps_the_inline_script_quoted() -> Result<(), RenderError> {
    for assignment in ["FOO-BAR=value", "=value"] {
        let rendered = join_argv_for_run(&argv(&[
            "env",
            "--",
            assignment,
            "bash",
            "-c",
            "echo $value",
        ]))?;
        assert_eq!(
            rendered,
            format!("env -- {assignment} bash -c 'echo $value'")
        );
    }
    let rendered = join_argv_for_run(&argv(&[
        "env",
        "--",
        "-",
        "FOO=bar",
        "bash",
        "-c",
        "echo $value",
    ]))?;
    assert_eq!(rendered, "env -- - FOO=bar bash -c 'echo $value'");
    Ok(())
}
