// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::declared_fixture::{TestResult, refusal, rendered, workflow};

fn job_running(job_keys: &str, command: &str, step_keys: &str) -> String {
    workflow(
        "",
        "",
        &format!(
            "\n[[workflow.job]]\nid = \"check\"\n{job_keys}\n[[workflow.job.step]]\nname = \"Run\"\nrun = '''{command}'''\n{step_keys}\n"
        ),
    )
}

const WRAPPED_HELPER: &[&str] = &[
    "env -C stage velnor-ci check",
    "env -C stage env -u A velnor-ci check",
    "env -u A velnor-ci check",
    "env -u A -C stage A=1 velnor-ci check",
    "env -i velnor-ci check",
    "env -iC stage velnor-ci check",
    "env --chdir=stage velnor-ci check",
    "env --chdir stage velnor-ci check",
    "env --unset=A velnor-ci check",
    "env -- velnor-ci check",
    "env -S 'velnor-ci check'",
    "A=1 env -C stage velnor-ci check",
    "/usr/bin/env -C stage velnor-ci check",
    "env -C stage /usr/local/bin/velnor-ci check",
];

#[test]
fn env_options_cannot_hide_a_helper_call() -> TestResult {
    for command in WRAPPED_HELPER.iter().copied() {
        let message = refusal(&job_running("", command, ""))
            .map_err(|error| format!("{command:?}: {error}"))?;
        assert!(message.contains("helper = true"), "{command:?}: {message}");
    }
    for command in WRAPPED_HELPER {
        let command = command.replace("check", "plan");
        let message = refusal(&job_running("helper = true", &command, ""))
            .map_err(|error| format!("{command:?}: {error}"))?;
        assert!(
            message.contains("must start with velnor-ci"),
            "{command:?}: {message}"
        );
    }
    Ok(())
}

#[test]
fn a_command_after_env_that_is_not_the_helper_needs_no_helper() -> TestResult {
    for command in [
        "env",
        "env -C stage ls",
        "env -C stage echo velnor-ci",
        "env -u velnor-ci echo ok",
        "env A=velnor-ci echo ok",
        "env -C stage ./velnor-ci --version",
        "./velnor-ci --version",
        "target/release/velnor-ci --version",
        "env -C stage target/velnor-ci --version",
    ] {
        rendered(&job_running("", command, "")).map_err(|error| format!("{command:?}: {error}"))?;
    }
    Ok(())
}

#[test]
fn a_path_outside_the_workspace_to_velnor_ci_is_still_the_helper() -> TestResult {
    for command in [
        "/usr/local/bin/velnor-ci check",
        "../velnor-ci check",
        "stage/../../velnor-ci check",
        "~/bin/velnor-ci check",
        "$HOME/bin/velnor-ci check",
    ] {
        let message = refusal(&job_running("", command, ""))
            .map_err(|error| format!("{command:?}: {error}"))?;
        assert!(message.contains("helper = true"), "{command:?}: {message}");
    }
    Ok(())
}

#[test]
fn env_and_assignments_cannot_hide_a_repository_script_from_the_write_token_rule() -> TestResult {
    let write = "permissions = { contents = \"write\" }";
    let token = "env = { GH_TOKEN = \"${{ github.token }}\" }";
    for command in [
        "env -C scripts ./publish.sh",
        "env -u A bash publish.sh",
        "env A=1 bash publish.sh",
        "A=1 ./publish.sh",
        "A=1 env -C scripts python3 publish.py",
        "'./publish.sh'",
    ] {
        let message = refusal(&job_running(write, command, token))
            .map_err(|error| format!("{command:?}: {error}"))?;
        assert!(message.contains("github.token"), "{command:?}: {message}");
    }
    for command in [
        "env -C dist gh release create v1",
        "A=1 gh release create v1",
    ] {
        rendered(&job_running(write, command, token))
            .map_err(|error| format!("{command:?}: {error}"))?;
    }
    Ok(())
}
