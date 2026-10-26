// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::{
    error::Error,
    fs, io,
    path::Path,
    process::{Command, Output},
};

use sha2::{Digest, Sha256};
use tempfile::TempDir;

#[path = "api2026_pull_context_fixture/git.rs"]
mod git;
#[path = "api2026_pull_context_fixture/provider.rs"]
mod provider;

#[derive(Clone, Copy)]
pub(crate) enum Scenario {
    #[cfg(not(debug_assertions))]
    ReleaseSentinel,
    Valid,
    ReversedParents,
    ChangedRef,
    MergePendingReady,
    MergePendingExhausted,
    MergePendingFalse,
    MergePendingChanged,
    MergePendingDeadline,
    MergePendingApiDeadline,
    HttpFailure,
    StdoutLimit,
    StderrLimit,
    Deadline,
}

impl Scenario {
    fn is_deadline_child(self) -> bool {
        matches!(
            self,
            Self::Deadline | Self::MergePendingDeadline | Self::MergePendingApiDeadline
        )
    }
}

pub(crate) const REPOSITORY_ID: u64 = 819_273;

pub(crate) fn run(scenario: Scenario, expected: &str) -> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let paths = Paths::new(&temp);
    fs::create_dir_all(&paths.repo)?;
    fs::create_dir_all(&paths.home)?;
    fs::create_dir_all(&paths.runner_temp)?;
    fs::write(paths.runner_temp.join("github-output"), "")?;
    let cli = Path::new(env!("CARGO_BIN_EXE_velnor-actions"));
    let (digest, commit) = manifest_identity(cli, scenario)?;
    let git = git::Fixture::new(&paths);
    let identities = git.initialize(&digest, &commit)?;
    write_event(&paths, &identities)?;
    provider::write(&paths, &identities, scenario)?;
    let output = invoke(&paths, &identities, scenario)?;
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(
        output.status.success(),
        scenario.is_deadline_child(),
        "unexpected fixture status: {stderr}"
    );
    let requests = match fs::read_to_string(paths.root.join("provider/requests.log")) {
        Ok(requests) => requests,
        #[cfg(not(debug_assertions))]
        Err(error)
            if matches!(scenario, Scenario::ReleaseSentinel)
                && error.kind() == io::ErrorKind::NotFound =>
        {
            String::new()
        }
        Err(error) => return Err(error.into()),
    };
    assert!(
        expected_error(scenario, &stderr, expected),
        "expected {expected}, got {stderr}; fixture routes: {requests}"
    );
    if matches!(
        scenario,
        Scenario::HttpFailure | Scenario::StdoutLimit | Scenario::StderrLimit | Scenario::Deadline
    ) {
        assert!(
            requests.contains("route=repos/test-owner/test-tap\n"),
            "GH failure boundary was not reached"
        );
        for text in [
            "package_publish_api_failed",
            "operation=repository",
            "cap=1048576",
            "elapsed_ms=",
            "timeout_ms=",
        ] {
            assert!(stderr.contains(text), "missing {text}: {stderr}");
        }
        for secret in [
            "fixture-secret-canary",
            "fixture-signed-canary",
            "redirect.invalid",
            "\u{1b}",
            "_velnor_read_nonce",
            "Accept:",
            "repos/test-owner/test-tap",
        ] {
            assert!(!stderr.contains(secret), "unsafe diagnostic field escaped");
        }
        if matches!(scenario, Scenario::HttpFailure) {
            assert!(
                stderr.contains("http=403") && stderr.contains("code=1"),
                "missing actual numeric HTTP/exit status: {stderr}"
            );
        }
    }
    assert!(!paths.runner_temp.join("velnor/local/plan.json").exists());
    assert!(
        !paths
            .runner_temp
            .join("velnor/request/plan-v1-response.json")
            .exists()
    );
    Ok(())
}

fn manifest_identity(cli: &Path, scenario: Scenario) -> Result<(String, String), Box<dyn Error>> {
    let digest = sha256(&fs::read(cli)?);
    Ok(match scenario {
        #[cfg(not(debug_assertions))]
        Scenario::ReleaseSentinel => {
            let incorrect = if digest == "1".repeat(64) {
                "2".repeat(64)
            } else {
                "1".repeat(64)
            };
            assert_ne!(incorrect, digest);
            (incorrect, "b".repeat(40))
        }
        _ => (
            digest,
            "6930000000000000000000000000000000000000".to_owned(),
        ),
    })
}

pub(crate) struct Paths {
    pub(crate) repo: std::path::PathBuf,
    pub(crate) home: std::path::PathBuf,
    pub(crate) runner_temp: std::path::PathBuf,
    pub(crate) root: std::path::PathBuf,
}

impl Paths {
    fn new(temp: &TempDir) -> Self {
        Self {
            repo: temp.path().join("repo"),
            home: temp.path().join("home"),
            runner_temp: temp.path().join("runner-temp"),
            root: temp.path().to_owned(),
        }
    }
}

pub(crate) struct Identities {
    pub(crate) base: String,
    pub(crate) head: String,
    pub(crate) merge: String,
    pub(crate) root_tree: String,
    pub(crate) vel_nor_tree: String,
}

fn write_event(paths: &Paths, ids: &Identities) -> Result<(), Box<dyn Error>> {
    let repo = serde_json::json!({"id": REPOSITORY_ID, "full_name": "test-owner/test-tap"});
    let payload = serde_json::json!({
        "repository": repo.clone(),
        "pull_request": {
            "number": 17,
            "base": {"sha": ids.base, "ref": "main", "repo": repo.clone()},
            "head": {"sha": ids.head, "ref": "fixture-head", "repo": {"id": REPOSITORY_ID, "full_name": "test-owner/test-tap", "fork": false}}
        }
    });
    fs::write(paths.root.join("event.json"), payload.to_string())?;
    Ok(())
}

fn invoke(paths: &Paths, ids: &Identities, scenario: Scenario) -> Result<Output, Box<dyn Error>> {
    let cli = env!("CARGO_BIN_EXE_velnor-actions");
    let path = format!("{}:/usr/bin:/bin", paths.root.join("bin").display());
    let request = paths
        .runner_temp
        .join("velnor/request/plan-v1-request.json");
    let request_parent = request
        .parent()
        .ok_or_else(|| io::Error::other("request parent missing"))?;
    fs::create_dir_all(request_parent)?;
    fs::write(
        &request,
        serde_json::json!({
            "schema":1,"op":"plan-v1","run_key":"local","event":"pull_request",
            "base":ids.base,"head":ids.merge,"root":paths.repo,"repository":"test-owner/test-tap"
        })
        .to_string(),
    )?;
    let is_deadline = scenario.is_deadline_child();
    let mut command = if is_deadline {
        let mut command = Command::new(std::env::current_exe()?);
        command.args([
            "--exact",
            "github_api_deadline_reports_safe_context",
            "--nocapture",
        ]);
        command
    } else {
        Command::new(cli)
    };
    let output = command
        .current_dir(&paths.repo)
        .env_clear()
        .env(
            "VELNOR_API_DIAGNOSTIC_CHILD",
            if is_deadline { "1" } else { "0" },
        )
        .env("PATH", path)
        .env("VELNOR_FIXTURE_STATE", paths.root.join("provider"))
        .env("VELNOR_FIXTURE_BASE", &ids.base)
        .env("VELNOR_FIXTURE_HEAD", &ids.head)
        .env("VELNOR_FIXTURE_MERGE", &ids.merge)
        .env("VELNOR_FIXTURE_ROOT_TREE", &ids.root_tree)
        .env("VELNOR_FIXTURE_VELNOR_TREE", &ids.vel_nor_tree)
        .env("HOME", &paths.home)
        .env("GH_TOKEN", "fixture_token")
        .env("RUNNER_TEMP", &paths.runner_temp)
        .env("GITHUB_OUTPUT", paths.runner_temp.join("github-output"))
        .env("GITHUB_RUN_ID", "123")
        .env("GITHUB_RUN_ATTEMPT", "1")
        .env("GITHUB_REPOSITORY", "test-owner/test-tap")
        .env("GITHUB_WORKSPACE", &paths.repo)
        .env("GITHUB_EVENT_NAME", "pull_request")
        .env("GITHUB_REF", "refs/pull/17/merge")
        .env("GITHUB_SHA", &ids.merge)
        .env("GITHUB_EVENT_PATH", paths.root.join("event.json"))
        .env("VELNOR_INTERNAL_OP", "plan-v1")
        .env(
            "VELNOR_REQUEST_FILE",
            paths
                .runner_temp
                .join("velnor/request/plan-v1-request.json"),
        )
        .output()?;
    Ok(output)
}

pub(crate) fn deadline_child() -> Result<bool, Box<dyn Error>> {
    if std::env::var("VELNOR_API_DIAGNOSTIC_CHILD").ok().as_deref() != Some("1") {
        return Ok(false);
    }
    let request = fs::read_to_string(std::env::var("VELNOR_REQUEST_FILE")?)?;
    let deadline = velnor_actions_mise::CheckDeadline::after(std::time::Duration::from_secs(5))?;
    let failure =
        fs::read_to_string(Path::new(&std::env::var("VELNOR_FIXTURE_STATE")?).join("failure"))?;
    let polling = matches!(failure.as_str(), "merge-deadline" | "merge-api-deadline");
    let expired = deadline.scope(|| {
        let result = velnor_actions_orchestrator::plan_internal(&request);
        assert!(result.is_err(), "deadline plan unexpectedly succeeded");
        if let Err(error) = result {
            eprintln!("{error}");
            let error_text = error.to_string();
            let accepted = if polling {
                polling_deadline_error(&error_text)
            } else {
                error_text.contains("category=timeout")
            };
            assert!(accepted, "transport timeout diagnostic missing: {error}");
        }
    });
    assert!(expired.is_err(), "public operation deadline did not expire");
    Ok(true)
}

fn sha256(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut text = String::with_capacity(64);
    for byte in Sha256::digest(bytes) {
        text.push(char::from(HEX[usize::from(byte >> 4)]));
        text.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    text
}

fn expected_error(scenario: Scenario, stderr: &str, expected: &str) -> bool {
    if matches!(
        scenario,
        Scenario::MergePendingDeadline | Scenario::MergePendingApiDeadline
    ) {
        stderr.lines().any(polling_deadline_error)
    } else {
        stderr.contains(expected)
    }
}

fn polling_deadline_error(error: &str) -> bool {
    error == "internal: package_public_verification_deadline"
        || error.starts_with(
            "internal: package_publish_api_failed: operation=pull_requests category=timeout ",
        )
}
