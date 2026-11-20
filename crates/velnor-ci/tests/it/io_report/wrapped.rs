// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::io::Repo;
use serde_json::json;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::process::Command;
use velnor_ci::report::LaneMetrics;

type ResultTest = Result<(), Box<dyn std::error::Error>>;

/// A fixture whose only Cargo root is `directory` (`.` for the checkout itself), and a
/// stand-in for `cargo` that prints one compiler artifact for its package.
fn fixture(name: &str, directory: &str) -> Result<(Repo, String), Box<dyn std::error::Error>> {
    let repo = Repo::new(name);
    let prefix = if directory == "." {
        String::new()
    } else {
        format!("{directory}/")
    };
    let manifest = format!("{prefix}Cargo.toml");
    let lock = format!("{prefix}Cargo.lock");
    let library = format!("{prefix}src/lib.rs");
    repo.write_all(&[
        (
            &manifest,
            "[package]\nname='demo'\nversion='0.1.0'\nedition='2024'\n",
        ),
        (&library, "pub fn value() {}\n"),
        (
            &lock,
            "version = 4\n[[package]]\nname='demo'\nversion='0.1.0'\n",
        ),
    ]);
    super::metadata_inputs(&repo)?;
    velnor_gen::io::write_metadata(repo.root())?;
    let package = fs::canonicalize(repo.root())?.join(&prefix);
    let package = package.to_string_lossy().trim_end_matches('/').to_owned();
    let artifact = json!({
        "reason": "compiler-artifact",
        "package_id": format!("path+file://{package}#demo@0.1.0"),
        "manifest_path": format!("{package}/Cargo.toml"),
        "target": {"name": "demo", "kind": ["lib"], "crate_types": ["lib"], "src_path": format!("{package}/src/lib.rs")},
        "profile": {"opt_level": "0", "debuginfo": 2, "debug_assertions": true, "overflow_checks": true, "test": false},
        "features": ["default"],
        "filenames": ["target/debug/deps/libdemo-opaque.rmeta"],
        "fresh": false
    });
    let program = repo.root().join("cargo-fixture");
    fs::write(
        &program,
        format!("#!/bin/sh\nprintf '%s\\n' '{artifact}'\n"),
    )?;
    fs::set_permissions(&program, fs::Permissions::from_mode(0o755))?;
    Ok((repo, program.to_string_lossy().into_owned()))
}

fn run(repo: &Repo, command: &[&str]) -> Result<LaneMetrics, Box<dyn std::error::Error>> {
    let mut arguments = vec!["run", "--"];
    arguments.extend_from_slice(command);
    let output = Command::new(env!("CARGO_BIN_EXE_velnor-ci"))
        .args(&arguments)
        .current_dir(repo.root())
        .env("RUNNER_TEMP", repo.root())
        .env("GITHUB_JOB", "rust")
        .env_remove("GITHUB_TOKEN")
        .env_remove("CARGO_TARGET_DIR")
        .output()?;
    assert!(output.status.success(), "{output:?}");
    let summary = repo.root().join("step-summary.md");
    let output = Command::new(env!("CARGO_BIN_EXE_velnor-ci"))
        .arg("report")
        .current_dir(repo.root())
        .env("RUNNER_TEMP", repo.root())
        .env("GITHUB_JOB", "rust")
        .env("GITHUB_STEP_SUMMARY", &summary)
        .env_remove("GITHUB_TOKEN")
        .env_remove("CARGO_TARGET_DIR")
        .output()?;
    assert!(output.status.success(), "{output:?}");
    let metrics: LaneMetrics =
        serde_json::from_slice(&fs::read(repo.root().join("ci-metrics.json"))?)?;
    Ok(metrics)
}

fn wrapped<'a>(program: &'a str, arguments: &[&'a str]) -> Vec<&'a str> {
    let mut command = vec![
        env!("CARGO_BIN_EXE_velnor-ci"),
        "inputs",
        "build",
        "--",
        program,
    ];
    command.extend_from_slice(arguments);
    command
}

fn assert_one_workspace_compilation(metrics: &LaneMetrics, root: &str) {
    assert_eq!(metrics.invocations.len(), 1, "{metrics:?}");
    let invocation = &metrics.invocations[0];
    assert_eq!(invocation.root, root, "{metrics:?}");
    assert_eq!(invocation.compilations.len(), 1, "{metrics:?}");
    assert!(invocation.compilations[0].workspace);
    assert!(
        metrics
            .summary()
            .contains("Compiled: 1 workspace, 0 dependency crates"),
        "{}",
        metrics.summary()
    );
}

#[test]
fn a_cargo_command_wrapped_by_inputs_build_is_recorded_for_the_checkout_root() -> ResultTest {
    let (repo, program) = fixture("report-wrapped-root", ".")?;
    let metrics = run(&repo, &wrapped(&program, &[]))?;
    assert_one_workspace_compilation(&metrics, ".");
    assert!(metrics.invocations[0].argv[0].ends_with("cargo-fixture"));
    Ok(())
}

#[test]
fn a_cargo_command_wrapped_by_inputs_build_is_recorded_for_a_nested_root() -> ResultTest {
    let (repo, program) = fixture("report-wrapped-nested", "lab")?;
    let metrics = run(
        &repo,
        &wrapped(&program, &["--manifest-path", "lab/Cargo.toml"]),
    )?;
    assert_one_workspace_compilation(&metrics, "lab");
    Ok(())
}

#[test]
fn a_bare_cargo_command_is_still_recorded_for_a_nested_root() -> ResultTest {
    let (repo, program) = fixture("report-bare-nested", "lab")?;
    let metrics = run(&repo, &[&program, "--manifest-path", "lab/Cargo.toml"])?;
    assert_one_workspace_compilation(&metrics, "lab");
    Ok(())
}
