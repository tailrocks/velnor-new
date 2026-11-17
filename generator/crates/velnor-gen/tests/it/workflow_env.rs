// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_gen::expected_files;
use velnor_model::RepoPath;

use crate::facts::TestResult;
use crate::golden::{load, samples};
use crate::pipeline::{view, workflow};

fn environment(text: &str) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
    let tree = crate::yaml_tree::document(text)?;
    tree.get("env")
        .cloned()
        .ok_or_else(|| "the generated workflow has no environment".into())
}

#[test]
fn the_generated_workflow_stops_the_toolchain_from_installing_itself() -> TestResult {
    let env = environment(&workflow(&view(&["Cargo.toml"], None)?)?)?;
    assert_eq!(
        env.get("RUSTUP_AUTO_INSTALL")
            .and_then(serde_json::Value::as_str),
        Some("0"),
        "rustup must not install a toolchain that the workflow did not pin"
    );
    assert_eq!(
        env.get("MISE_AUTO_INSTALL")
            .and_then(serde_json::Value::as_str),
        Some("false"),
        "mise must not install a tool that the workflow did not pin"
    );
    Ok(())
}

#[test]
fn the_generated_setup_still_reads_the_repository_mise_configuration() -> TestResult {
    let text = workflow(&view(&["Cargo.toml"], None)?)?;
    let env = environment(&text)?;
    for flag in [
        "MISE_NO_CONFIG",
        "MISE_NO_ENV",
        "MISE_NO_HOOKS",
        "MISE_LOCKFILE",
    ] {
        assert!(
            env.get(flag).is_none(),
            "{flag} would hide the repository's mise configuration from the generated setup"
        );
    }
    assert!(
        text.contains("uses: jdx/mise-action@"),
        "the generated setup must keep installing the pinned tools through mise"
    );
    Ok(())
}

#[test]
fn the_pinned_nextest_stays_installable_by_the_generated_setup() -> TestResult {
    let sample = load(&samples().join("velnor-new"))?;
    let mise = sample
        .files
        .get(&RepoPath::new("mise.toml")?)
        .ok_or("the sample has no mise configuration")?;
    let mise = String::from_utf8_lossy(mise);
    assert!(
        mise.contains("cargo-nextest"),
        "the sample does not pin nextest"
    );
    let files = expected_files(&sample)?;
    let text = files
        .get(&RepoPath::new(".github/workflows/rust-ci.yml")?)
        .ok_or("no generated workflow")?;
    assert!(
        text.contains("uses: jdx/mise-action@"),
        "the generated setup must keep installing the pinned tools through mise"
    );
    assert!(
        text.contains("cargo-nextest nextest run"),
        "the generated workflow must run the pinned nextest"
    );
    let env = environment(text)?;
    assert!(
        env.get("MISE_NO_CONFIG").is_none(),
        "the mise shim must keep resolving the pinned nextest from the repository's configuration"
    );
    Ok(())
}
