// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_gen::{GenerateError, expected_files};
use velnor_model::RepoPath;
use velnor_rust::DetectError;

use crate::archive::jobs_of;
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

fn lane_step_order(profile: &str) -> Result<(usize, usize), Box<dyn std::error::Error>> {
    let file = RepoPath::new("rust-toolchain.toml")?;
    let mut declared = view(&["Cargo.toml", "lab/Cargo.toml"], None)?;
    declared.tracked_paths.insert(file.clone());
    declared.files.insert(
        file,
        format!("[toolchain]\nchannel = \"1.99.1\"\nprofile = \"{profile}\"\n").into_bytes(),
    );
    let jobs = jobs_of(&workflow(&declared)?);
    let (_, lane) = jobs
        .iter()
        .find(|(id, _)| id == "rust-lab")
        .ok_or("no lab lane")?;
    let names: Vec<&str> = lane
        .lines()
        .filter_map(|line| {
            line.strip_prefix("      - name: ")
                .or_else(|| line.strip_prefix("        name: "))
        })
        .collect();
    let at = |name: &str| names.iter().position(|found| *found == name);
    let save = at("Save the tools cache").ok_or("no tools save")?;
    let prepare = at("velnor-ci prepare").ok_or("no prepare")?;
    Ok((save, prepare))
}

#[test]
fn a_complete_profile_installs_its_components_before_the_tools_cache_is_saved() -> TestResult {
    let (save, prepare) = lane_step_order("complete")?;
    assert!(prepare < save, "prepare at {prepare}, tools save at {save}");
    Ok(())
}

#[test]
fn a_default_or_minimal_profile_keeps_saving_the_tools_cache_before_prepare() -> TestResult {
    for profile in ["default", "minimal"] {
        let (save, prepare) = lane_step_order(profile)?;
        assert!(
            save < prepare,
            "{profile}: tools save at {save}, prepare at {prepare}"
        );
    }
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
