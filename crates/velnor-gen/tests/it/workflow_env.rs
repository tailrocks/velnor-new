// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::facts::TestResult;
use crate::pipeline::{view, workflow};

fn rendered() -> Result<String, Box<dyn std::error::Error>> {
    workflow(&view(&["Cargo.toml"], None)?)
}

#[test]
fn the_generated_workflow_keeps_rustup_and_mise_from_installing_an_unpinned_toolchain() -> TestResult
{
    let text = rendered()?;
    let tree = crate::yaml_tree::document(&text)?;
    let environment = tree.get("env").cloned().unwrap_or_default();
    assert_eq!(
        environment
            .get("RUSTUP_AUTO_INSTALL")
            .and_then(serde_json::Value::as_str),
        Some("0"),
        "the generated workflow must not let rustup install a toolchain it did not pin: {environment}"
    );
    assert_eq!(
        environment
            .get("MISE_AUTO_INSTALL")
            .and_then(serde_json::Value::as_str),
        Some("false"),
        "the generated workflow must not let mise install a tool it did not pin: {environment}"
    );
    for name in ["RUSTUP_AUTO_INSTALL", "MISE_AUTO_INSTALL"] {
        assert_eq!(
            text.matches(name).count(),
            1,
            "{name} must be set once, at the workflow level, so no job can turn the automatic installation back on"
        );
    }
    Ok(())
}

#[test]
fn the_generated_workflow_never_hides_the_repository_mise_configuration() -> TestResult {
    let text = rendered()?;
    for (flag, consequence) in [
        (
            "MISE_NO_CONFIG",
            "mise would stop reading the repository's mise configuration, and the pinned tools with it",
        ),
        (
            "MISE_NO_ENV",
            "mise would stop reading the environment the repository's mise configuration declares",
        ),
        (
            "MISE_NO_HOOKS",
            "mise would stop running the hooks the repository's mise configuration declares",
        ),
        (
            "MISE_LOCKFILE",
            "mise would stop reading the pinned versions in the repository's mise lock file",
        ),
    ] {
        assert!(
            !text.contains(flag),
            "{flag} must not reach the generated workflow: {consequence}"
        );
    }
    Ok(())
}
