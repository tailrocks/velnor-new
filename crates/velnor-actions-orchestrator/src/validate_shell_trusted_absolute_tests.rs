use super::{ShellDialect, bodies};

#[test]
fn renderer_trusted_absolute_shell_is_exactly_admitted() -> Result<(), String> {
    let trusted = format!(
        "jobs:\n  probe:\n    runs-on: ubuntu-26.04\n    steps:\n      - name: verifier\n        run: echo verified\n        shell: {}\n",
        velnor_actions_workflow_renderer::VERIFY_SHELL
    );
    assert_eq!(
        bodies(&trusted)?,
        [("echo verified".to_owned(), ShellDialect::Bash)]
    );

    for altered in [
        velnor_actions_workflow_renderer::VERIFY_SHELL.replace("/bin/bash", "/usr/bin/bash"),
        velnor_actions_workflow_renderer::VERIFY_SHELL.replace("--noprofile", "--login"),
        velnor_actions_workflow_renderer::VERIFY_SHELL.replace("{0}", "{1}"),
    ] {
        let workflow = format!(
            "jobs:\n  probe:\n    runs-on: ubuntu-26.04\n    steps:\n      - name: verifier\n        run: echo verified\n        shell: {altered}\n"
        );
        assert!(
            bodies(&workflow).is_err_and(|err| err.contains("shell_form_unsupported")),
            "altered renderer shell accepted: {altered}"
        );
    }
    Ok(())
}
