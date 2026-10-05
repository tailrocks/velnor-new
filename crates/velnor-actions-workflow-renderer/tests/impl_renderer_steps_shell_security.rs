//! Inline-shell command-substitution rejection cases.
use velnor_actions_workflow_renderer::validate_command_argv;

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
