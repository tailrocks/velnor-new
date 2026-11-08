// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::*;

fn fixture_event(root: &Path, source: &str) -> Result<std::path::PathBuf, Box<dyn Error>> {
    let path = root.join("github-event.json");
    let event = serde_json::json!({"inputs": {"plan": format!("release-{}", &source[..12]), "source_sha": source, "version": "0.1.1"}});
    std::fs::write(&path, serde_json::to_vec(&event)?)?;
    Ok(path)
}

pub(super) fn rehearse_journal_start(root: &Path) -> Result<(), Box<dyn Error>> {
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

pub(super) fn rehearse_journal_coordinate(root: &Path) -> Result<(), Box<dyn Error>> {
    if std::env::var_os("VELNOR_NATIVE_TAMPER_CARGO").is_some() {
        let cargo = root.join("velnor/rustup/toolchains/1.99.1-x86_64-unknown-linux-gnu/bin/cargo");
        let mut bytes = std::fs::read(&cargo)?;
        *bytes.last_mut().ok_or("empty Cargo executable")? ^= 1;
        std::fs::write(cargo, bytes)?;
    }
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
        if std::env::var_os("VELNOR_NATIVE_NEW_RUN_PHASE1").is_some() {
            return Ok(());
        }
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

pub(super) fn rehearse_candidate_denial() -> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let root = temp.path();
    let source = "a".repeat(40);
    let sha = committed_intent(root, &source, true)?;
    let github_event = fixture_event(root, &source)?;
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
        format!(
            "#!/bin/sh\nexport VELNOR_TEST_COMMAND_ARGS=\"$*\"\nexec \"$VELNOR_TEST_BINARY\" --exact {CANDIDATE_CASE}\n"
        ),
    )?;
    std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o755))?;
    let script = generated_script("release-preflight", "Release candidate-receipt")?;
    let output = std::process::Command::new("bash")
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
        .env("GITHUB_EVENT_PATH", &github_event)
        .env("GITHUB_RUN_ID", "12")
        .env("GITHUB_RUN_ATTEMPT", "1")
        .env("GIT_TOKEN", "fixture")
        .output()?;
    done.store(true, Ordering::Release);
    server
        .join()
        .map_err(|_| "fake GitHub panicked")?
        .map_err(|e| format!("fake GitHub: {e}"))?;
    assert!(output.status.success());
    crate::libtest_case::assert_one_passed(&output);
    assert!(!root.join("velnor-crate-release-receipt.json").exists());
    Ok(())
}

pub(super) fn rehearse_source_require(event: &str, denied: bool) -> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let root = temp.path();
    let source = "a".repeat(40);
    let sha = committed_intent(root, &source, event == "workflow_dispatch")?;
    let github_event = fixture_event(root, &source)?;
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
        format!(
            "#!/bin/sh\nexport VELNOR_TEST_COMMAND_ARGS=\"$*\"\nexec \"$VELNOR_TEST_BINARY\" --exact {SOURCE_CASE}\n"
        ),
    )?;
    std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o755))?;
    let child = std::process::Command::new("bash")
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
        .env("GITHUB_EVENT_PATH", &github_event)
        .env("GITHUB_RUN_ID", if event == "push" { "13" } else { "12" })
        .env("GITHUB_RUN_ATTEMPT", "1")
        .env("GIT_TOKEN", "fixture")
        .output()?;
    done.store(true, Ordering::Release);
    server
        .join()
        .map_err(|_| "fake GitHub panicked")?
        .map_err(|e| format!("fake GitHub: {e}"))?;
    let actual = std::fs::read_to_string(output)?;
    if denied {
        assert!(!child.status.success());
        assert!(
            String::from_utf8_lossy(&child.stdout)
                .contains("test result: FAILED. 0 passed; 1 failed;")
        );
        assert_eq!(actual, "");
        return Ok(());
    }
    assert!(child.status.success());
    crate::libtest_case::assert_one_passed(&child);
    assert!(actual.contains("eligible=true\n"));
    let expected = if event == "push" {
        sha.as_str()
    } else {
        "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
    };
    assert!(actual.contains(&format!("source_sha={expected}\n")));
    Ok(())
}
