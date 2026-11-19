// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;
use std::fs;

use velnor_actions_workflow_renderer::VERIFY_VERIFIER_SHELL;

use super::{ShellDialect, StagedRun, staged_runs};
use crate::OrchestratorError;

const RETIRED_VERIFIER_SHELL: &str = "/bin/bash --noprofile --norc -e -o pipefail {0}";

fn staged(shell: &str) -> Result<Vec<StagedRun>, OrchestratorError> {
    let staging =
        tempfile::tempdir().map_err(|err| OrchestratorError::io("staging", err.to_string()))?;
    let workflow = format!(
        "name: probe\non: push\njobs:\n  probe:\n    runs-on: ubuntu-latest\n    steps:\n      - name: Probe\n        shell: {shell}\n        run: echo probe\n"
    );
    fs::write(staging.path().join("probe.yml"), workflow)
        .map_err(|err| OrchestratorError::io("probe.yml", err.to_string()))?;
    staged_runs(staging.path(), &["probe.yml".to_owned()])
}

#[test]
fn only_the_hermetic_verifier_shell_is_accepted_among_verifier_shells() -> Result<(), Box<dyn Error>>
{
    for shell in ["bash", VERIFY_VERIFIER_SHELL] {
        let runs = staged(shell)?;
        assert_eq!(runs.len(), 1, "{shell}");
        assert_eq!(runs[0].shell, ShellDialect::Bash, "{shell}");
    }
    let rejected = staged(RETIRED_VERIFIER_SHELL);
    assert!(
        matches!(
            &rejected,
            Err(OrchestratorError::Validation { tool, problem })
                if tool == "shellcheck" && problem == "shell_form_unsupported"
        ),
        "{rejected:?}"
    );
    Ok(())
}

fn staged_yaml(workflow: &str) -> Result<Vec<StagedRun>, OrchestratorError> {
    let staging =
        tempfile::tempdir().map_err(|err| OrchestratorError::io("staging", err.to_string()))?;
    fs::write(staging.path().join("probe.yml"), workflow)
        .map_err(|err| OrchestratorError::io("probe.yml", err.to_string()))?;
    staged_runs(staging.path(), &["probe.yml".to_owned()])
}

fn helper_workflow(env: &str, run: &str) -> String {
    format!(
        "jobs:\n  probe:\n    runs-on: ubuntu-latest\n    steps:\n      - name: Probe\n        shell: bash\n        env:\n{env}\n        run: {run}\n"
    )
}

fn assert_shell_rejected(workflow: &str) {
    let result = staged_yaml(workflow);
    assert!(
        matches!(&result, Err(OrchestratorError::Validation { tool, .. }) if tool == "shellcheck"),
        "{result:?}"
    );
}

#[test]
fn helper_env_scalar_aliases_preserve_all_staged_shell_bodies() -> Result<(), Box<dyn Error>> {
    let mut workflow = helper_workflow(
        "          VELNOR_HELPER_LAUNCHER: &velnor_0 'printf ''%s'' \"$HOME\"; $(echo x) & !'\n          VELNOR_HELPER_ARGS: *velnor_0\n          VELNOR_HELPER_SHA256: &velnor_1 'abcdef'",
        "\"echo first\"",
    );
    workflow.push_str(
        "      - name: Reuse\n        shell: bash\n        env:\n          VELNOR_HELPER_LAUNCHER: *velnor_0\n        run: \"echo second\"\n  other:\n    runs-on: ubuntu-latest\n    steps:\n      - name: Across jobs\n        shell: bash\n        env:\n          VELNOR_HELPER_LAUNCHER: *velnor_0\n        run: \"echo third\"\n",
    );
    let runs = staged_yaml(&workflow)?;
    assert_eq!(
        runs,
        ["echo first", "echo second", "echo third"].map(|body| StagedRun {
            body: body.to_owned(),
            shell: ShellDialect::Bash
        })
    );
    Ok(())
}

#[test]
fn helper_env_scalar_aliases_reject_ambiguous_shapes_and_namespaces() {
    for env in [
        "          VELNOR_HELPER: *forward",
        "          VELNOR_HELPER: &cycle *cycle",
        "          VELNOR_HELPER: &tag !tag 'body'",
        "          VELNOR_HELPER: !tag &tag 'body'",
        "          VELNOR_HELPER: &block |",
        "          VELNOR_HELPER: &flow ['body']",
        "          VELNOR_HELPER: &flow {'key': 'body'}",
        "          VELNOR_HELPER: &quote 'unpaired'quote'",
        "          VELNOR_HELPER: &children 'body'\n            NESTED: other",
        "          VELNOR_HELPER: &same 'body'\n          VELNOR_HELPER_ARGS: &same 'other'",
        "          VELNOR_HELPER: &value 'body'\n          <<: *value",
        "          VELNOR_HELPER: &value 'body'\n          VELNOR_HELPER_ARGS: [*value]",
    ] {
        assert_shell_rejected(&helper_workflow(env, "echo probe"));
    }
    for workflow in [
        helper_workflow("          VELNOR_HELPER: &value 'body'", "*value"),
        helper_workflow("          VELNOR_HELPER: *value", "echo probe").replace(
            "        env:",
            "        run: &value echo probe\n        env:",
        ),
        helper_workflow(
            "          VELNOR_HELPER: &value 'body'",
            "&value echo probe",
        ),
        helper_workflow("          VELNOR_HELPER: &value 'body'", "echo probe")
            .replace("        env:", "        env: &value"),
        helper_workflow("          VELNOR_HELPER: &value 'body'", "echo probe")
            .replace("        env:", "        with: *value\n        env:"),
        helper_workflow("          VELNOR_HELPER: *value", "echo probe").replace(
            "        env:",
            "        with: &value\n          INPUT: body\n        env:",
        ),
        helper_workflow("          VELNOR_HELPER: &value 'body'", "echo probe")
            .replace("        env:", "        with:"),
        helper_workflow("          OTHER: &value 'body'", "echo probe"),
        helper_workflow("            VELNOR_HELPER: &value 'body'", "echo probe"),
        helper_workflow("          VELNOR_HELPER: &value 'body'", "echo probe")
            .replace("        env:", "    env:"),
    ] {
        assert_shell_rejected(&workflow);
    }
}

#[test]
fn helper_env_scalar_alias_expansion_remains_bounded() {
    let scalar = "x".repeat(100_000);
    let mut workflow = helper_workflow(
        &format!("          VELNOR_HELPER: &large '{scalar}'"),
        "echo probe",
    );
    for _ in 0..180 {
        workflow.push_str(
            "      - name: Reuse\n        shell: bash\n        env:\n          VELNOR_HELPER: *large\n        run: echo probe\n",
        );
    }
    assert!(workflow.len() < 500_000);
    assert_shell_rejected(&workflow);
}
