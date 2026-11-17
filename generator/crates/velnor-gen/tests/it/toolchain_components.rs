// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_gen::{GenerateError, expected_files};
use velnor_model::RepoPath;
use velnor_rust::DetectError;

use crate::facts::TestResult;
use crate::pipeline::{view, workflow};

#[test]
fn every_job_adds_the_components_and_targets_the_toolchain_file_declares() -> TestResult {
    let file = RepoPath::new("rust-toolchain.toml")?;
    let mut declared = view(&["Cargo.toml"], Some(b"shape = \"lint-test\"\n"))?;
    declared.tracked_paths.insert(file.clone());
    declared.files.insert(
        file,
        b"[toolchain]\nchannel = \"1.99.1\"\ncomponents = [\"llvm-tools\"]\ntargets = [\"wasm32-unknown-unknown\"]\n"
            .to_vec(),
    );
    let cargo_config = RepoPath::new(".cargo/config.toml")?;
    declared.tracked_paths.insert(cargo_config.clone());
    declared.files.insert(
        cargo_config,
        b"[build]\ntarget = \"wasm32-unknown-unknown\"\n".to_vec(),
    );
    let text = workflow(&declared)?;
    let runs: Vec<&str> = text
        .lines()
        .filter_map(|line| line.trim().strip_prefix("run: rustup "))
        .collect();
    assert_eq!(
        runs,
        [
            "component add clippy llvm-tools rustfmt",
            "target add wasm32-unknown-unknown",
            "component add clippy llvm-tools rustfmt",
            "target add wasm32-unknown-unknown",
            "component add clippy llvm-tools rustfmt",
            "target add wasm32-unknown-unknown",
        ]
    );
    Ok(())
}

fn profile_workflow(profile: &str) -> Result<String, Box<dyn std::error::Error>> {
    let file = RepoPath::new("rust-toolchain.toml")?;
    let mut declared = view(&["Cargo.toml"], Some(b"shape = \"lint-test\"\n"))?;
    declared.tracked_paths.insert(file.clone());
    declared.files.insert(
        file,
        format!("[toolchain]\nchannel = \"1.99.1\"\nprofile = \"{profile}\"\n").into_bytes(),
    );
    workflow(&declared)
}

#[test]
fn a_default_profile_adds_its_docs_and_a_minimal_one_adds_nothing() -> TestResult {
    let default = profile_workflow("default")?;
    assert!(
        default.contains("run: rustup component add clippy rust-docs rustfmt"),
        "{default}"
    );
    let minimal = profile_workflow("minimal")?;
    assert!(
        minimal.contains("run: rustup component add clippy rustfmt\n"),
        "{minimal}"
    );
    assert!(!minimal.contains("rust-docs"));
    Ok(())
}

#[test]
fn a_profile_rustup_does_not_know_is_an_error() -> TestResult {
    let file = RepoPath::new("rust-toolchain.toml")?;
    let mut declared = view(&["Cargo.toml"], None)?;
    declared.tracked_paths.insert(file.clone());
    declared.files.insert(
        file,
        b"[toolchain]\nchannel = \"1.99.1\"\nprofile = \"everything\"\n".to_vec(),
    );
    assert!(matches!(
        expected_files(&declared),
        Err(GenerateError::Detect(DetectError::Invalid { .. }))
    ));
    Ok(())
}
