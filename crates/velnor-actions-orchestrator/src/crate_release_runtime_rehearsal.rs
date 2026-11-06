// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;
use std::io::Write as _;
use std::os::unix::fs::PermissionsExt as _;
use std::path::Path;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::time::{Duration, Instant};

#[test]
fn qualified_runtime_rejects_missing_git_without_fallback() -> Result<(), Box<dyn Error>> {
    if std::env::var_os("VELNOR_MISSING_GIT_CHILD").is_some() {
        assert!(
            matches!(super::run_qualified_crate_release("source-require"),
            Err(crate::OrchestratorError::Internal { problem }) if problem == "release_git_bundle_invalid")
        );
        return Ok(());
    }
    let temp = tempfile::tempdir()?;
    let status = std::process::Command::new(std::env::current_exe()?)
        .arg("--exact")
        .arg("crate_release_runtime::rehearsal::qualified_runtime_rejects_missing_git_without_fallback")
        .env("VELNOR_MISSING_GIT_CHILD", "1")
        .env("RUNNER_TEMP", temp.path())
        .env("GITHUB_WORKSPACE", temp.path())
        .status()?;
    assert!(status.success());
    Ok(())
}

#[test]
fn generated_source_require_rehearsal() -> Result<(), Box<dyn Error>> {
    if std::env::var_os("VELNOR_SOURCE_CHILD").is_some() {
        assert_eq!(
            std::env::var("VELNOR_TEST_COMMAND_ARGS")?,
            "crate-release source-require"
        );
        return super::run_qualified_with_fixture_git("source-require").map_err(Into::into);
    }
    for event in ["push", "workflow_dispatch"] {
        rehearse_source_require(event, false)?;
        rehearse_source_require(event, true)?;
    }
    Ok(())
}

#[test]
fn generated_candidate_receipt_rejects_unprotected_environment() -> Result<(), Box<dyn Error>> {
    if let Some(temp) = std::env::var_os("VELNOR_NATIVE_ACQUIRE_GIT") {
        let root = std::path::PathBuf::from(temp);
        let git = crate::crate_release_git_bundle::acquire(
            &root,
            velnor_actions_mise::CheckDeadline::after(Duration::from_secs(600))?,
        )?;
        assert_eq!(git.exec_path(), root.join("velnor/git-bundle"));
        return Ok(());
    }
    if std::env::var_os("VELNOR_NATIVE_RECEIPT_CHILD").is_some() {
        let operation = std::env::var("VELNOR_NATIVE_OPERATION")?;
        assert!(matches!(
            operation.as_str(),
            "candidate-receipt" | "verify-admission" | "journal-start" | "journal-coordinate"
        ));
        assert_eq!(
            std::env::var("VELNOR_TEST_COMMAND_ARGS")?,
            format!("crate-release {operation}")
        );
        assert!(matches!(super::run_crate_release(&operation),
            Err(crate::OrchestratorError::Internal { problem }) if problem == "crate_release_authority_not_qualified"));
        if operation == "journal-coordinate"
            && std::env::var_os("VELNOR_NATIVE_NO_TOKEN_ON_RETRY").is_some()
            && std::env::var_os("VELNOR_NATIVE_PID_FILE")
                .is_some_and(|path| Path::new(&path).exists())
        {
            assert!(std::env::var_os("ACTIONS_ID_TOKEN_REQUEST_TOKEN").is_none());
            assert!(std::env::var_os("ACTIONS_ID_TOKEN_REQUEST_URL").is_none());
        }
        if operation == "journal-coordinate"
            && let Some(path) = std::env::var_os("VELNOR_NATIVE_PID_FILE")
        {
            std::fs::write(path, std::process::id().to_string())?;
        }
        return super::run_qualified_crate_release(&operation).map_err(Into::into);
    }
    if std::env::var_os("VELNOR_CANDIDATE_CHILD").is_some() {
        assert_eq!(
            std::env::var("VELNOR_TEST_COMMAND_ARGS")?,
            "crate-release candidate-receipt"
        );
        assert!(matches!(super::run_crate_release("candidate-receipt"),
            Err(crate::OrchestratorError::Internal { problem }) if problem == "crate_release_authority_not_qualified"));
        assert!(
            matches!(super::run_qualified_with_fixture_git("candidate-receipt"),
            Err(crate::OrchestratorError::Internal { problem }) if problem == "crate_release_environment_bypass")
        );
        return Ok(());
    }
    rehearse_candidate_denial()?;
    if std::env::var_os("VELNOR_TEST_NATIVE_REHEARSAL").is_some() {
        rehearse_supplied_candidate()?;
    }
    Ok(())
}

fn rehearse_supplied_candidate() -> Result<(), Box<dyn Error>> {
    let root = std::path::PathBuf::from(std::env::var_os("RUNNER_TEMP").ok_or("runner temp")?);
    let bin = root.join("velnor/bin");
    std::fs::create_dir_all(&bin)?;
    let executable = bin.join(format!("velnor-actions-{}", env!("CARGO_PKG_VERSION")));
    std::fs::write(
        &executable,
        "#!/bin/sh\nexport VELNOR_TEST_COMMAND_ARGS=\"$*\"\nexec \"$VELNOR_TEST_BINARY\" --exact crate_release_runtime::rehearsal::generated_candidate_receipt_rejects_unprotected_environment\n",
    )?;
    std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o755))?;
    let output = supplied_step(&root, "release-preflight", "candidate-receipt")?;
    let receipt = root.join("velnor-crate-release-receipt.json");
    if std::env::var("VELNOR_NATIVE_EXPECT_SUCCESS")? != "1" {
        assert!(!output.status.success());
        let actual = String::from_utf8_lossy(&output.stdout).to_string()
            + &String::from_utf8_lossy(&output.stderr);
        assert!(
            actual.contains(&std::env::var("VELNOR_NATIVE_EXPECT_ERROR")?),
            "{actual}"
        );
        assert!(!receipt.exists());
        return Ok(());
    }
    assert!(
        output.status.success(),
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let receipt: crate::crate_release_controller::CrateReleaseAdmissionReceipt =
        serde_json::from_slice(&std::fs::read(receipt)?)?;
    assert_eq!(
        receipt.github.request.workflow_sha,
        std::env::var("GITHUB_SHA")?
    );
    assert_eq!(
        receipt.github.request.source_sha,
        std::env::var("VELNOR_NATIVE_EXPECT_SOURCE")?
    );
    assert_eq!(receipt.packages.len(), 1);
    assert_eq!(
        receipt.packages[0].source.name,
        std::env::var("VELNOR_NATIVE_EXPECT_PACKAGE")?
    );
    assert_eq!(
        receipt.packages[0].source.version,
        std::env::var("VELNOR_NATIVE_EXPECT_VERSION")?
    );
    assert!(
        matches!(receipt.semver_evidence.get(&receipt.packages[0].source.name),
        Some(crate::crate_release_semver::SemverEvidence::Checked { baseline, baseline_checksum, checker_sha256 })
        if baseline == &std::env::var("VELNOR_NATIVE_EXPECT_BASELINE")?
        && baseline_checksum == &std::env::var("VELNOR_NATIVE_EXPECT_CHECKSUM")?
        && checker_sha256 == velnor_actions_mise::catalog::qualified_cargo_semver_checks("ubuntu-26.04").ok_or("checker")?.executable_sha256)
    );
    if verify_supplied_admission(&root)? && std::env::var_os("VELNOR_NATIVE_JOURNAL").is_some() {
        rehearse_journal_start(&root)?;
        if std::env::var_os("VELNOR_NATIVE_COORDINATE").is_some() {
            rehearse_journal_coordinate(&root)?;
        }
    }
    Ok(())
}

fn verify_supplied_admission(root: &Path) -> Result<bool, Box<dyn Error>> {
    if std::env::var_os("VELNOR_NATIVE_MUTATE_AFTER_RECEIPT").is_some() {
        let workspace =
            std::path::PathBuf::from(std::env::var_os("GITHUB_WORKSPACE").ok_or("workspace")?);
        std::fs::write(
            workspace.join("release-source/src/lib.rs"),
            b"pub fn changed() {}\n",
        )?;
    }
    let output_path = root.join("github-output");
    std::fs::write(&output_path, "")?;
    let output = supplied_step(root, "release-publish", "verify-admission")?;
    let actual = String::from_utf8_lossy(&output.stdout).to_string()
        + &String::from_utf8_lossy(&output.stderr);
    let expected_error = std::env::var("VELNOR_NATIVE_VERIFY_ERROR")?;
    if expected_error.is_empty() {
        assert!(output.status.success(), "{actual}");
        assert_eq!(
            std::fs::read_to_string(output_path)?,
            "release_mode=publish\n"
        );
    } else {
        assert!(!output.status.success());
        assert!(actual.contains(&expected_error), "{actual}");
        assert_eq!(std::fs::read_to_string(output_path)?, "");
        return Ok(false);
    }
    Ok(true)
}

fn rehearse_journal_start(root: &Path) -> Result<(), Box<dyn Error>> {
    let output = supplied_step(root, "release-publish", "journal-start")?;
    let actual = String::from_utf8_lossy(&output.stdout).to_string()
        + &String::from_utf8_lossy(&output.stderr);
    let error = std::env::var("VELNOR_NATIVE_JOURNAL_ERROR")?;
    if error.is_empty() {
        assert!(output.status.success(), "{actual}");
    } else {
        assert!(!output.status.success());
        assert!(actual.contains(&error), "{actual}");
    }
    Ok(())
}

fn rehearse_journal_coordinate(root: &Path) -> Result<(), Box<dyn Error>> {
    if std::env::var_os("VELNOR_NATIVE_POISON_CHECKER").is_some() {
        std::fs::remove_file(root.join("velnor-semver/cargo-semver-checks"))?;
    }
    if std::env::var_os("VELNOR_NATIVE_COORDINATE_CRASH").is_some() {
        use std::os::unix::process::ExitStatusExt as _;
        let interrupted = supplied_step(root, "release-publish", "journal-coordinate")?;
        assert!(
            interrupted.status.signal() == Some(9) || interrupted.status.code() == Some(137),
            "{}{}",
            String::from_utf8_lossy(&interrupted.stdout),
            String::from_utf8_lossy(&interrupted.stderr)
        );
    }
    let output = supplied_step(root, "release-publish", "journal-coordinate")?;
    let actual = String::from_utf8_lossy(&output.stdout).to_string()
        + &String::from_utf8_lossy(&output.stderr);
    let error = std::env::var("VELNOR_NATIVE_COORDINATE_ERROR")?;
    if error.is_empty() {
        assert!(output.status.success(), "{actual}");
    } else {
        assert!(!output.status.success());
        assert!(actual.contains(&error), "{actual}");
    }
    Ok(())
}

fn supplied_step(
    root: &Path,
    job: &str,
    operation: &str,
) -> Result<std::process::Output, Box<dyn Error>> {
    let script = generated_script(job, &format!("Release {operation}"))?;
    let mut command = std::process::Command::new("bash");
    command
        .args(["-e", "-c", &script])
        .env("VELNOR_NATIVE_RECEIPT_CHILD", "1")
        .env("VELNOR_NATIVE_OPERATION", operation)
        .env("VELNOR_TEST_BINARY", std::env::current_exe()?)
        .env("GITHUB_OUTPUT", root.join("github-output"))
        .env("RUSTUP_HOME", root.join("velnor/rustup"))
        .env("MISE_RUSTUP_HOME", root.join("velnor/rustup"))
        .env("CARGO_HOME", root.join("velnor/cargo"))
        .env("MISE_CARGO_HOME", root.join("velnor/cargo"))
        .env("RUSTUP_AUTO_INSTALL", "0");
    if matches!(operation, "journal-start" | "journal-coordinate") {
        command.env("HOME", root).env("GH_TOKEN", "fixture");
    }
    if operation == "journal-coordinate"
        && std::env::var_os("VELNOR_NATIVE_NO_TOKEN_ON_RETRY").is_some()
        && root.join("coordinator.pid").exists()
    {
        command.env_remove("ACTIONS_ID_TOKEN_REQUEST_TOKEN");
        command.env_remove("ACTIONS_ID_TOKEN_REQUEST_URL");
    }
    Ok(command.output()?)
}

fn rehearse_candidate_denial() -> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let root = temp.path();
    let source = "a".repeat(40);
    let sha = committed_intent(root, &source, true)?;
    let listener = std::net::TcpListener::bind("127.0.0.1:0")?;
    listener.set_nonblocking(true)?;
    let origin = format!("http://{}", listener.local_addr()?);
    let done = Arc::new(AtomicBool::new(false));
    let server_done = Arc::clone(&done);
    let server_sha = sha.clone();
    let server = std::thread::spawn(move || {
        serve_admission(&listener, &server_done, &server_sha, &source, true)
    });
    let bin = root.join("velnor/bin");
    std::fs::create_dir_all(&bin)?;
    let executable = bin.join(format!("velnor-actions-{}", env!("CARGO_PKG_VERSION")));
    std::fs::write(
        &executable,
        "#!/bin/sh\nexport VELNOR_TEST_COMMAND_ARGS=\"$*\"\nexec \"$VELNOR_TEST_BINARY\" --exact crate_release_runtime::rehearsal::generated_candidate_receipt_rejects_unprotected_environment\n",
    )?;
    std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o755))?;
    let script = generated_script("release-preflight", "Release candidate-receipt")?;
    let status = std::process::Command::new("bash")
        .args(["-e", "-c", &script])
        .env("VELNOR_CANDIDATE_CHILD", "1")
        .env("VELNOR_TEST_BINARY", std::env::current_exe()?)
        .env("VELNOR_TEST_GITHUB_ORIGIN", origin)
        .env("RUNNER_TEMP", root)
        .env("GITHUB_WORKSPACE", root)
        .env("GITHUB_REPOSITORY", "owner/project")
        .env("GITHUB_SHA", &sha)
        .env("GITHUB_WORKFLOW_SHA", &sha)
        .env(
            "GITHUB_WORKFLOW_REF",
            "owner/project/.github/workflows/release.yml@refs/heads/main",
        )
        .env("GITHUB_REF", "refs/heads/main")
        .env("GITHUB_EVENT_NAME", "workflow_dispatch")
        .env("GITHUB_RUN_ID", "12")
        .env("GITHUB_RUN_ATTEMPT", "1")
        .env("GIT_TOKEN", "fixture")
        .status()?;
    done.store(true, Ordering::Release);
    server
        .join()
        .map_err(|_| "fake GitHub panicked")?
        .map_err(|e| format!("fake GitHub: {e}"))?;
    assert!(status.success());
    assert!(!root.join("velnor-crate-release-receipt.json").exists());
    Ok(())
}

fn rehearse_source_require(event: &str, denied: bool) -> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let root = temp.path();
    let source = "a".repeat(40);
    let sha = committed_intent(root, &source, event == "workflow_dispatch")?;
    let listener = std::net::TcpListener::bind("127.0.0.1:0")?;
    listener.set_nonblocking(true)?;
    let origin = format!("http://{}", listener.local_addr()?);
    let done = Arc::new(AtomicBool::new(false));
    let server_done = Arc::clone(&done);
    let server_sha = sha.clone();
    let server = std::thread::spawn(move || {
        serve_admission(&listener, &server_done, &server_sha, &source, denied)
    });
    let output = root.join("github-output");
    std::fs::write(&output, "")?;
    let script = source_require_script()?;
    let bin = root.join("velnor/bin");
    std::fs::create_dir_all(&bin)?;
    let executable = bin.join(format!("velnor-actions-{}", env!("CARGO_PKG_VERSION")));
    std::fs::write(
        &executable,
        "#!/bin/sh\nexport VELNOR_TEST_COMMAND_ARGS=\"$*\"\nexec \"$VELNOR_TEST_BINARY\" --exact crate_release_runtime::rehearsal::generated_source_require_rehearsal\n",
    )?;
    std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o755))?;
    let status = std::process::Command::new("bash")
        .args(["-e", "-c", &script])
        .env("VELNOR_SOURCE_CHILD", "1")
        .env("VELNOR_TEST_BINARY", std::env::current_exe()?)
        .env("VELNOR_TEST_GITHUB_ORIGIN", origin)
        .env("RUNNER_TEMP", root)
        .env("GITHUB_WORKSPACE", root)
        .env("GITHUB_OUTPUT", &output)
        .env("GITHUB_REPOSITORY", "owner/project")
        .env("GITHUB_SHA", &sha)
        .env("GITHUB_WORKFLOW_SHA", &sha)
        .env(
            "GITHUB_WORKFLOW_REF",
            "owner/project/.github/workflows/release.yml@refs/heads/main",
        )
        .env("GITHUB_REF", "refs/heads/main")
        .env("GITHUB_EVENT_NAME", event)
        .env("GITHUB_RUN_ID", if event == "push" { "13" } else { "12" })
        .env("GITHUB_RUN_ATTEMPT", "1")
        .env("GIT_TOKEN", "fixture")
        .status()?;
    done.store(true, Ordering::Release);
    server
        .join()
        .map_err(|_| "fake GitHub panicked")?
        .map_err(|e| format!("fake GitHub: {e}"))?;
    let actual = std::fs::read_to_string(output)?;
    if denied {
        assert!(!status.success());
        assert_eq!(actual, "");
        return Ok(());
    }
    assert!(status.success());
    assert!(actual.contains("eligible=true\n"));
    let expected = if event == "push" {
        sha.as_str()
    } else {
        "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
    };
    assert!(actual.contains(&format!("source_sha={expected}\n")));
    Ok(())
}

include!("crate_release_runtime_rehearsal_support.rs");
