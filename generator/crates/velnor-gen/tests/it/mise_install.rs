// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_model::RepoView;
use velnor_rust::detect;

use crate::facts::{TestResult, lib, repo};

fn locked_mise_view(mise: &str) -> Result<RepoView, Box<dyn std::error::Error>> {
    repo(
        vec![("Cargo.toml", vec![lib("app", "Cargo.toml")?])],
        &[
            ("mise.toml", mise),
            ("mise.lock", "[[tools.rust]]\nversion = \"1.99.1\"\n"),
        ],
    )
}

#[test]
fn plan_rejects_a_tool_template_even_when_it_renders_the_locked_version() -> TestResult {
    let view = locked_mise_view("[tools]\nrust = \"{{ exec(command='true') }}1.99.1\"\n")?;
    assert!(detect(&view).is_err());
    Ok(())
}

#[test]
fn plan_rejects_a_plugin_that_can_replace_the_locked_tool() -> TestResult {
    let view = locked_mise_view(
        "[tools]\nrust = \"1.99.1\"\n[plugins]\nrust = \"https://github.com/attacker/mise-rust\"\n",
    )?;
    assert!(detect(&view).is_err());
    Ok(())
}

#[test]
fn plan_rejects_a_variable_template_that_executes_before_installation() -> TestResult {
    let view = locked_mise_view(
        "[tools]\nrust = \"1.99.1\"\n[vars]\npayload = \"{{ exec(command='true') }}\"\n",
    )?;
    assert!(detect(&view).is_err());
    Ok(())
}

#[test]
fn plan_accepts_a_plain_tool_version_with_its_matching_lock() -> TestResult {
    let facts = detect(&locked_mise_view("[tools]\nrust = \"1.99.1\"\n")?)?;
    assert_eq!(facts.tools.tools["rust"].requested, ["1.99.1"]);
    assert_eq!(facts.tools.tools["rust"].locked, ["1.99.1"]);
    Ok(())
}
