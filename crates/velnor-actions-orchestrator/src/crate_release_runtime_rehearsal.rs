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
        let git = crate::crate_release_git_bundle::acquire(&root)?;
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
        && checker_sha256 == velnor_actions_mise::catalog::qualified_cargo_semver_checks("ubuntu-24.04").ok_or("checker")?.executable_sha256)
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
    if std::env::var_os("VELNOR_NATIVE_COORDINATE_CRASH").is_some() {
        use std::os::unix::process::ExitStatusExt as _;
        let interrupted = supplied_step(root, "release-publish", "journal-coordinate")?;
        assert!(interrupted.status.signal() == Some(9) || interrupted.status.code() == Some(137));
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
    Ok(command.output()?)
}

fn rehearse_candidate_denial() -> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let root = temp.path();
    let source = "a".repeat(40);
    let sha = committed_intent(root, &source)?;
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
    let sha = committed_intent(root, &source)?;
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

fn committed_intent(root: &Path, source: &str) -> Result<String, Box<dyn Error>> {
    let intent: velnor_actions_contract::config::CrateReleaseIntent = serde_json::from_value(
        serde_json::json!({"schema":1,"repository":"owner/project","default_branch":"main","environment":"release","required_workflow_path":".github/workflows/ci.yml","trusted_reviewer_id":139_017,"rust_version":"1.97.1","generator_version":"0.1.5","generator_source_sha":"f1041f322c54cd7edfdef06afd307799eeaa801a","generator_manifest_sha256":"e38d047150f9e0347ec2160fd65bb7f026842e6f416e456990cf3b7ccd284c52","release_plz_version":"0.4.3","runner":"ubuntu-24.04","authentication":"trusted-publishing","manifest_path":"Cargo.toml","pr_write_paths":["CHANGELOG.md","Cargo.lock","Cargo.toml"],"registry":"crates-io","packages":["example"],"publish_features":{"example":[]},"tag_template":"{{ package }}-v{{ version }}","recovery":{"package":"example","version":"0.1.1","source_sha":source}}),
    )?;
    std::fs::create_dir(root.join(".github"))?;
    std::fs::write(
        root.join(".github/velnor-release-intent.toml"),
        toml::to_string(&intent)?,
    )?;
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
    git(root, &["rev-parse", "HEAD"])
}

fn git(root: &Path, args: &[&str]) -> Result<String, Box<dyn Error>> {
    let result = std::process::Command::new("git")
        .current_dir(root)
        .args(args)
        .output()?;
    if !result.status.success() {
        return Err(String::from_utf8_lossy(&result.stderr).into_owned().into());
    }
    Ok(String::from_utf8(result.stdout)?.trim().to_owned())
}

fn source_require_script() -> Result<String, Box<dyn Error>> {
    generated_script("release-publish", "Release source-require")
}

fn generated_script(job: &str, name: &str) -> Result<String, Box<dyn Error>> {
    let (spec, rust) = crate::release_steps::tests::fixture()?;
    let workflow = velnor_actions_workflow_renderer::release_tree::render_release_workflow(
        &spec,
        &velnor_actions_workflow_renderer::release_tree::ReleaseRenderContext {
            generator_version: env!("CARGO_PKG_VERSION").into(),
            runs_on: "ubuntu-24.04".into(),
        },
        &rust,
    )?;
    let workflow: serde_json::Value = serde_saphyr::from_str(&workflow)?;
    let steps = workflow["jobs"][job]["steps"]
        .as_array()
        .ok_or("publisher steps")?;
    Ok(steps
        .iter()
        .find(|step| step["name"].as_str() == Some(name))
        .and_then(|step| step["run"].as_str())
        .ok_or("source-require step")?
        .to_owned())
}

fn serve_admission(
    listener: &std::net::TcpListener,
    done: &AtomicBool,
    policy: &str,
    source: &str,
    denied: bool,
) -> Result<(), String> {
    let deadline = Instant::now() + Duration::from_secs(30);
    let mut count = 0;
    while !done.load(Ordering::Acquire) && Instant::now() < deadline {
        let (mut stream, _) = match listener.accept() {
            Ok(connection) => connection,
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                std::thread::sleep(Duration::from_millis(10));
                continue;
            }
            Err(error) => return Err(error.to_string()),
        };
        stream.set_nonblocking(false).map_err(|e| e.to_string())?;
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .map_err(|e| e.to_string())?;
        let (line, _) = super::tests::read_test_request(&mut stream)?;
        let mut value = if line == "GET /repos/owner/project HTTP/1.1" {
            serde_json::json!({"id":22,"full_name":"owner/project","default_branch":"main"})
        } else {
            super::tests::test_github_admission_response(&line, policy, source)
                .ok_or_else(|| format!("unexpected request {line}"))?
        };
        if denied && line.starts_with("GET /repos/owner/project/environments/release ") {
            value["can_admins_bypass"] = serde_json::json!(true);
        }
        let body = value.to_string();
        write!(stream, "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).map_err(|e| e.to_string())?;
        count += 1;
    }
    if count == 0 {
        return Err("no admission requests".into());
    }
    Ok(())
}
