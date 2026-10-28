// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use serde_json::Value;
use tempfile::{Builder, TempDir};

use super::mutation::copy_mutation_repository;
use super::support::{
    Harness, copy_executable, copy_tree, diagnostics_candidate, retained_workspace, sha256,
};

#[test]
fn replaces_external_original_after_nested_plan_and_keeps_all_goldens_matching()
-> Result<(), Box<dyn Error>> {
    let harness = Harness::new()?;
    let diagnostic = diagnostics_candidate()?;
    let (_external_dir, candidate) = external_candidate(&harness, &diagnostic)?;
    let admitted_sha256 = sha256(&fs::read(&candidate)?);
    let source = harness.directory.join("source");
    copy_mutation_repository(&harness.root, &source)?;
    add_fixture_origin(&source)?;
    let oracle = copy_oracle(&harness)?;
    let output = run_fault(
        &harness,
        &source,
        &candidate,
        &oracle,
        "replace-original-after-nested-plan",
        &candidate,
    )?;
    retain_test_fixture(harness);

    assert_eq!(
        output.status.code(),
        Some(0),
        "collector failed after the external original changed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    for case in [
        "nested",
        "mbx-nextest",
        "empty-suite",
        "minimal-cargo",
        "dogfood",
    ] {
        assert!(
            stdout.contains(&format!("match: {case}")),
            "missing match for {case}"
        );
    }
    assert!(stdout.contains("ALL GOLDENS MATCH"));
    assert_eq!(fs::read(&candidate)?, b"not a valid executable\n");
    let workspace =
        retained_workspace(&output).ok_or("successful evidence workspace was not retained")?;
    let version = workspace.join("raw/version");
    assert!(
        String::from_utf8_lossy(&fs::read(version.join("version.raw.txt"))?)
            .starts_with("velnor-actions ")
    );
    assert_eq!(fs::read_to_string(version.join("version.status"))?, "0\n");
    let plan = workspace.join("raw/nested");
    assert_ne!(fs::metadata(plan.join("plan.raw.txt"))?.len(), 0);
    assert_eq!(fs::read_to_string(plan.join("plan.status"))?, "0\n");
    assert_eq!(
        fs::read_to_string(plan.join("plan.termination"))?,
        "completed\n"
    );
    let copied_cli = workspace.join("cli/velnor-actions");
    assert_eq!(sha256(&fs::read(copied_cli)?), admitted_sha256);
    assert_manifest_digest(
        &workspace.join("nested/.velnor/release-manifest.json"),
        target_for_host()?,
        &admitted_sha256,
    )?;
    Ok(())
}

#[test]
fn replacing_current_executable_after_version_stops_before_fixture_commands()
-> Result<(), Box<dyn Error>> {
    let harness = Harness::new()?;
    let diagnostic = diagnostics_candidate()?;
    let (_external_dir, candidate) = external_candidate(&harness, &diagnostic)?;
    let original_sha256 = sha256(&fs::read(&candidate)?);
    let oracle = copy_oracle(&harness)?;
    let output = run_fault(
        &harness,
        &harness.root,
        &candidate,
        &oracle,
        "replace-current-after-version",
        &candidate,
    )?;
    retain_test_fixture(harness);
    let workspace =
        retained_workspace(&output).ok_or("failed evidence workspace was not retained")?;
    let stderr = String::from_utf8_lossy(&output.stderr);
    let identity_rejected = stderr.contains("candidate executable identity changed");
    let plan_absent = !workspace.join("raw/nested/plan.status").exists()
        && !workspace.join("stage/nested/plan.exit").exists();
    assert!(
        output.status.code() == Some(2) && identity_rejected && plan_absent,
        "expected identity rejection before plan; status={:?}, identity_rejected={identity_rejected}, plan_absent={plan_absent}, stderr={stderr}",
        output.status.code()
    );
    let version = workspace.join("raw/version");
    assert!(
        String::from_utf8_lossy(&fs::read(version.join("version.raw.txt"))?)
            .starts_with("velnor-actions ")
    );
    assert_eq!(fs::read_to_string(version.join("version.status"))?, "0\n");
    assert_eq!(
        fs::read_to_string(version.join("version.termination"))?,
        "completed\n"
    );
    let copied_cli = workspace.join("cli/velnor-actions");
    assert_eq!(fs::read(copied_cli)?, b"not a valid executable\n");
    assert_eq!(sha256(&fs::read(&candidate)?), original_sha256);
    assert!(!workspace.join("raw/nested").exists());
    assert!(!workspace.join("stage/nested").exists());
    Ok(())
}

#[test]
fn replacing_current_executable_after_nested_plan_stops_before_generate()
-> Result<(), Box<dyn Error>> {
    let harness = Harness::new()?;
    let diagnostic = diagnostics_candidate()?;
    let (_external_dir, candidate) = external_candidate(&harness, &diagnostic)?;
    let original_sha256 = sha256(&fs::read(&candidate)?);
    let oracle = copy_oracle(&harness)?;
    let output = run_fault(
        &harness,
        &harness.root,
        &candidate,
        &oracle,
        "replace-current-after-nested-plan",
        &candidate,
    )?;
    retain_test_fixture(harness);
    let workspace =
        retained_workspace(&output).ok_or("failed evidence workspace was not retained")?;
    let plan = workspace.join("raw/nested");
    assert_ne!(fs::metadata(plan.join("plan.raw.txt"))?.len(), 0);
    assert_eq!(fs::read_to_string(plan.join("plan.status"))?, "0\n");
    assert_eq!(
        fs::read_to_string(plan.join("plan.termination"))?,
        "completed\n"
    );
    assert_eq!(
        fs::read_to_string(workspace.join("stage/nested/plan.exit"))?,
        "0\n"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    let identity_rejected = stderr.contains("candidate executable identity changed");
    let generate_absent = !plan.join("generate.status").exists()
        && !plan.join("generate.raw.txt").exists()
        && !workspace.join("stage/nested/generate.exit").exists();
    assert!(
        output.status.code() == Some(2) && identity_rejected && generate_absent,
        "expected identity rejection before generate; status={:?}, identity_rejected={identity_rejected}, generate_absent={generate_absent}, stderr={stderr}",
        output.status.code()
    );
    assert_eq!(
        fs::read(workspace.join("cli/velnor-actions"))?,
        b"not a valid executable\n"
    );
    assert_eq!(sha256(&fs::read(&candidate)?), original_sha256);
    Ok(())
}

fn run_fault(
    harness: &Harness,
    root: &Path,
    candidate: &Path,
    oracle: &Path,
    mode: &str,
    fault_target: &Path,
) -> Result<Output, Box<dyn Error>> {
    let mut command = harness.command(
        "check",
        root,
        candidate,
        &harness.archive_guard,
        &harness.guard_sha256,
        Some(oracle),
    );
    command
        .env("TMPDIR", &harness.directory)
        .env("VELNOR_GOLDEN_KEEP_WORK", "1")
        .env("VELNOR_PLAN_DIAGNOSTICS_MODE", mode)
        .env("VELNOR_PLAN_DIAGNOSTICS_ROOT", root)
        .env("VELNOR_PLAN_DIAGNOSTICS_MUTATION_ROOT", &harness.directory)
        .env("VELNOR_PLAN_DIAGNOSTICS_EXECUTABLE_TARGET", fault_target);
    Ok(command.output()?)
}

fn add_fixture_origin(root: &Path) -> Result<(), Box<dyn Error>> {
    let output = Command::new("git")
        .args([
            "remote",
            "add",
            "origin",
            "https://github.com/tailrocks/velnor-new.git",
        ])
        .current_dir(root)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_TERMINAL_PROMPT", "0")
        .output()?;
    if output.status.success() {
        return Ok(());
    }
    Err(format!(
        "could not configure Velnor source fixture origin: {}",
        String::from_utf8_lossy(&output.stderr).trim_end()
    )
    .into())
}

fn retain_test_fixture(harness: Harness) {
    if std::env::var_os("VELNOR_GOLDEN_KEEP_WORK").is_some() {
        let directory = harness.temp.keep();
        eprintln!(
            "retaining executable custody test fixture: {}",
            directory.display()
        );
    }
}

fn external_candidate(
    harness: &Harness,
    diagnostic: &Path,
) -> Result<(TempDir, PathBuf), Box<dyn Error>> {
    let parent = harness
        .directory
        .parent()
        .ok_or("harness parent is missing")?;
    let temp = Builder::new()
        .prefix("diagnostics-cli-original.")
        .tempdir_in(parent)?;
    fs::set_permissions(temp.path(), fs::Permissions::from_mode(0o700))?;
    let path = temp.path().join("velnor-actions");
    copy_executable(diagnostic, &path)?;
    Ok((temp, path))
}

fn copy_oracle(harness: &Harness) -> Result<PathBuf, Box<dyn Error>> {
    let source = harness.root.join("docs/proposed/opentofu-goldens");
    let destination = harness.directory.join("oracle");
    copy_tree(&source, &destination)?;
    Ok(destination)
}

fn assert_manifest_digest(
    path: &Path,
    target: &str,
    expected_sha256: &str,
) -> Result<(), Box<dyn Error>> {
    let manifest: Value = serde_json::from_slice(&fs::read(path)?)?;
    let targets = manifest["targets"]
        .as_array()
        .ok_or("release manifest targets are missing")?;
    let selected = targets
        .iter()
        .filter(|entry| entry["target"].as_str() == Some(target))
        .collect::<Vec<_>>();
    assert_eq!(selected.len(), 1);
    assert_eq!(selected[0]["sha256"].as_str(), Some(expected_sha256));
    Ok(())
}

fn target_for_host() -> Result<&'static str, Box<dyn Error>> {
    match (std::env::consts::ARCH, std::env::consts::OS) {
        ("x86_64", "linux") => Ok("x86_64-unknown-linux-gnu"),
        ("aarch64", "macos") => Ok("aarch64-apple-darwin"),
        ("x86_64", "macos") => Ok("x86_64-apple-darwin"),
        _ => Err("unsupported host for the qualification fixture".into()),
    }
}
