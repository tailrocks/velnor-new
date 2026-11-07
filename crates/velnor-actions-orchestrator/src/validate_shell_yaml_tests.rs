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
