// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;

use velnor_actions_orchestrator::{prepare, render_staged_tree};

use super::workflow_configuration_fixture;

const LINT_CONFIG_PATH: &str = ".github/jactionlint.yaml";

fn lint_config_for(extra_lint_keys: &str) -> Result<String, Box<dyn Error>> {
    let config = format!(
        "schema = 1\n[workflow]\nname = \"CI\"\ndefault_branch = \"testmain\"\n\n[workflow.lint]\nprofile = \"default\"\n{extra_lint_keys}"
    );
    let repo = workflow_configuration_fixture::make_repo(&config)?;
    let prepared = prepare(repo.path())?;
    let rendered = render_staged_tree(&prepared)?;
    Ok(rendered
        .get(LINT_CONFIG_PATH)
        .ok_or("missing lint config")?
        .to_string())
}

#[test]
fn declared_lint_variables_reach_the_generated_lint_config() -> Result<(), Box<dyn Error>> {
    let declared = lint_config_for("variables = [\"VELNOR_CACHE_DEBUG\", \"A_FLAG\"]\n")?;
    assert!(
        declared.contains("config-variables:\n    - A_FLAG\n    - VELNOR_CACHE_DEBUG\n")
            || declared.contains("config-variables:\n  - A_FLAG\n  - VELNOR_CACHE_DEBUG\n"),
        "sorted declared variables missing: {declared}"
    );
    let none = lint_config_for("")?;
    assert!(
        none.contains("config-variables: []\n"),
        "undeclared repository must keep an empty list: {none}"
    );
    Ok(())
}

#[test]
fn malformed_lint_variables_are_rejected() -> Result<(), Box<dyn Error>> {
    for bad in ["GITHUB_TOKEN", "has space", ""] {
        let config = format!(
            "schema = 1\n[workflow]\nname = \"CI\"\ndefault_branch = \"testmain\"\n\n[workflow.lint]\nvariables = [\"{bad}\"]\n"
        );
        let repo = workflow_configuration_fixture::make_repo(&config)?;
        let outcome = prepare(repo.path()).and_then(|prepared| render_staged_tree(&prepared));
        assert!(outcome.is_err(), "variable {bad:?} unexpectedly accepted");
    }
    Ok(())
}
