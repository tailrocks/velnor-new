// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::{
    error::Error, fs, os::unix::fs::PermissionsExt, path::Path, process::Command, time::Duration,
};

use velnor_actions_mise::CheckDeadline;
use velnor_actions_orchestrator::{ValidateJobCandidateRequest, validate_job_candidate};

fn run(command: &mut Command) -> Result<(), Box<dyn Error>> {
    let output = command.output()?;
    if !output.status.success() {
        return Err(format!(
            "command failed: {}",
            String::from_utf8_lossy(&output.stderr)
        )
        .into());
    }
    Ok(())
}

fn git(root: &Path, args: &[&str]) -> Result<(), Box<dyn Error>> {
    run(Command::new("git")
        .current_dir(root)
        .args(args)
        .env("GIT_CONFIG_NOSYSTEM", "1"))
}

fn direct_cargo() -> Result<std::path::PathBuf, Box<dyn Error>> {
    let output = Command::new("rustc")
        .args(["--print", "sysroot"])
        .output()?;
    if !output.status.success() {
        return Err("rustc sysroot lookup failed".into());
    }
    Ok(Path::new(std::str::from_utf8(&output.stdout)?.trim()).join("bin/cargo"))
}

fn cargo_build(cargo: &Path, root: &Path, target: Option<&Path>) -> Result<(), Box<dyn Error>> {
    let mut command = Command::new(cargo);
    command
        .current_dir(root)
        .args([
            "build",
            "--locked",
            "--offline",
            "--manifest-path",
            "crates/velnor-runner/Cargo.toml",
        ])
        .env_remove("CARGO_TARGET_DIR")
        .env_remove("CARGO_BUILD_TARGET_DIR")
        .env_remove("RUSTC_WRAPPER")
        .env_remove("RUSTC_WORKSPACE_WRAPPER");
    if let Some(target) = target {
        command.env("CARGO_TARGET_DIR", target);
    }
    run(&mut command)
}

fn guard_error(root: &Path, base: &Path) -> Result<String, Box<dyn Error>> {
    let request = ValidateJobCandidateRequest {
        root: root.to_owned(),
        archive_path: base.join("missing.tar"),
        record_path: base.join("missing.json"),
        source_closure_path: base.join("closure.json"),
        deadline: CheckDeadline::after(Duration::from_secs(30))?,
    };
    let error = validate_job_candidate(&request)
        .err()
        .ok_or("invalid closure was accepted")?;
    Ok(error.to_string())
}

#[test]
fn native_image_guard_accepts_external_cargo_outputs_and_rejects_checkout_outputs()
-> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let root = temp.path().join("checkout");
    let nested = root.join("crates/velnor-runner");
    fs::create_dir_all(nested.join("src"))?;
    fs::write(
        root.join(".gitignore"),
        fs::read(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../.gitignore"))?,
    )?;
    fs::write(
        nested.join("Cargo.toml"),
        "[package]\nname = \"probe-fixture\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
    )?;
    fs::write(
        nested.join("Cargo.lock"),
        "version = 4\n\n[[package]]\nname = \"probe-fixture\"\nversion = \"0.1.0\"\n",
    )?;
    fs::write(nested.join("src/main.rs"), "fn main() {}\n")?;
    fs::write(temp.path().join("closure.json"), "{}")?;
    git(&root, &["init", "-q"])?;
    git(&root, &["add", "."])?;
    git(
        &root,
        &[
            "-c",
            "user.name=Fixture",
            "-c",
            "user.email=fixture@example.invalid",
            "-c",
            "commit.gpgsign=false",
            "commit",
            "-qm",
            "fixture",
        ],
    )?;
    let cargo = direct_cargo()?;
    cargo_build(&cargo, &root, None)?;
    assert!(guard_error(&root, temp.path())?.contains("image_candidate_untracked_files"));
    fs::remove_dir_all(nested.join("target"))?;
    cargo_build(
        &cargo,
        &root,
        Some(&temp.path().join("velnor-ci-image-target")),
    )?;
    assert!(guard_error(&root, temp.path())?.contains("image_candidate_closure_invalid"));
    Ok(())
}

#[test]
fn native_image_scripts_reject_missing_or_untrusted_target_before_building()
-> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let root = temp.path().join("checkout");
    let runner_temp = temp.path().join("runner-temp");
    fs::create_dir_all(&root)?;
    fs::create_dir_all(&runner_temp)?;
    fs::write(root.join("tracked"), "fixture\n")?;
    let head = commit_fixture(&root)?;
    let bin = temp.path().join("bin");
    fs::create_dir(&bin)?;
    let cargo = bin.join("cargo");
    fs::write(
        &cargo,
        "#!/bin/sh\nprintf invoked > \"$PROBE_CARGO_MARKER\"\nexit 77\n",
    )?;
    let docker = bin.join("docker");
    fs::write(&docker, "#!/bin/sh\nexit 77\n")?;
    fs::set_permissions(&cargo, fs::Permissions::from_mode(0o755))?;
    fs::set_permissions(&docker, fs::Permissions::from_mode(0o755))?;
    let marker = temp.path().join("cargo-invoked");
    let repository = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    for script in [
        "scripts/ci-native-image-tool-build.sh",
        "scripts/ci-native-image-candidates.sh",
    ] {
        for target in [None, Some(root.join("target"))] {
            let mut command = Command::new("bash");
            command
                .arg(repository.join(script))
                .arg("cargo")
                .current_dir(&root)
                .env("GITHUB_WORKSPACE", &root)
                .env("RUNNER_TEMP", &runner_temp)
                .env("GITHUB_SHA", &head)
                .env("GITHUB_WORKFLOW_SHA", &head)
                .env("GITHUB_EVENT_NAME", "push")
                .env("VELNOR_NATIVE_IMAGE_ACTIONS", "/usr/bin/true")
                .env("PATH", format!("{}:/usr/bin:/bin", bin.display()))
                .env("PROBE_CARGO_MARKER", &marker)
                .env_remove("CARGO_TARGET_DIR");
            if let Some(target) = target {
                command.env("CARGO_TARGET_DIR", target);
            }
            let output = command.output()?;
            assert!(
                !output.status.success(),
                "{script} accepted an untrusted target"
            );
            assert!(
                !marker.exists(),
                "{script} reached Cargo before validating the target"
            );
            assert!(
                !runner_temp.join("velnor-ci-image-candidates").exists(),
                "{script} created candidate state before validating the target"
            );
            assert!(
                String::from_utf8_lossy(&output.stderr).contains("CARGO_TARGET_DIR"),
                "{script} failed before checking its target: {}",
                String::from_utf8_lossy(&output.stderr)
            );
            assert!(!root.join("target").exists());
            assert!(!root.join("crates/velnor-runner/target").exists());
        }
    }
    Ok(())
}

fn commit_fixture(root: &Path) -> Result<String, Box<dyn Error>> {
    git(root, &["init", "-q"])?;
    git(root, &["add", "."])?;
    git(
        root,
        &[
            "-c",
            "user.name=Fixture",
            "-c",
            "user.email=fixture@example.invalid",
            "-c",
            "commit.gpgsign=false",
            "commit",
            "-qm",
            "fixture",
        ],
    )?;
    let head = Command::new("git")
        .current_dir(root)
        .args(["rev-parse", "HEAD"])
        .output()?;
    if !head.status.success() {
        return Err("fixture HEAD lookup failed".into());
    }
    Ok(std::str::from_utf8(&head.stdout)?.trim().to_owned())
}
