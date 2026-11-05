// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::{
    env,
    ffi::OsString,
    fs,
    path::{Path, PathBuf},
    process::{Command, ExitStatus},
};

use super::{seed_policy, workflow_run_event};

#[path = "preview_source_selection_fixture/server.rs"]
mod server;

pub(crate) const REPOSITORY: &str = "test-owner/test-tap";
pub(crate) const REPOSITORY_ID: u64 = 819_273;
pub(crate) const SOURCE_SHA: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
pub(crate) const AUTHORITY_SHA: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
pub(crate) const PROTECTED_SHA: &str = "cccccccccccccccccccccccccccccccccccccccc";
pub(crate) const CI_RUN_ID: u64 = 100;
const PREVIEW_RUN_ID: u64 = 200;
const PREVIEW_WORKFLOW_PATH: &str = ".github/workflows/binary-preview.yml";

pub(crate) type TestResult<T> = Result<T, Box<dyn std::error::Error>>;

pub(crate) struct Invocation {
    pub(crate) status: ExitStatus,
    pub(crate) stdout: Vec<u8>,
    pub(crate) stderr: Vec<u8>,
    pub(crate) output: String,
    pub(crate) requests: String,
    pub(crate) gh_calls: String,
}

struct PreparedFixture {
    runner_temp: PathBuf,
    home: PathBuf,
    mise_data_dir: PathBuf,
    path: OsString,
    event_path: PathBuf,
    github_output: PathBuf,
    policy_path: PathBuf,
    policy_text: String,
    request_log: PathBuf,
}

pub(crate) fn enter(test: &str) -> TestResult<bool> {
    if env::var_os("VELNOR_GH_FIXTURE_WORKER").is_some() {
        super::worker::validate_worker("preview", Some(test))?;
        return Ok(true);
    }
    let status = Command::new("/usr/bin/python3")
        .arg("-I")
        .arg(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/api2026_pull_context_fixture/driver.py"
        ))
        .args([
            "preview",
            test,
            if cfg!(debug_assertions) {
                "debug"
            } else {
                "release"
            },
        ])
        .status()?;
    if !status.success() {
        return Err(format!("isolated preview fixture failed: {status}").into());
    }
    Ok(false)
}

pub(crate) fn invoke(scenario: &str) -> TestResult<Invocation> {
    let temp = tempfile::tempdir()?;
    let fixture = prepare_fixture(temp.path())?;
    let _server = server::start(temp.path(), scenario)?;
    run_cli(&fixture)
}

fn prepare_fixture(root: &Path) -> TestResult<PreparedFixture> {
    let runner_temp = root.join("runner");
    let policy_dir = runner_temp.join("velnor");
    let home = root.join("home");
    let mise_data_dir =
        PathBuf::from(env::var_os("MISE_DATA_DIR").ok_or("qualified gh data directory missing")?);
    fs::create_dir_all(&policy_dir)?;
    fs::create_dir_all(&home)?;

    let policy = seed_policy().canonical_bytes()?;
    let policy_path = policy_dir.join("preview-source-seed-policy.json");
    fs::write(&policy_path, &policy)?;
    let event_path = root.join("workflow-run-event.json");
    fs::write(&event_path, serde_json::to_vec(&workflow_run_event())?)?;
    let github_output = root.join("github-output");
    fs::write(&github_output, b"")?;
    let request_log = root.join("requests.tsv");
    fs::write(&request_log, b"")?;
    let path = OsString::from("/usr/bin:/bin");
    Ok(PreparedFixture {
        runner_temp,
        home,
        mise_data_dir,
        path,
        event_path,
        github_output,
        policy_path,
        policy_text: String::from_utf8(policy)?,
        request_log,
    })
}

fn run_cli(fixture: &PreparedFixture) -> TestResult<Invocation> {
    let output = Command::new(env!("CARGO_BIN_EXE_velnor-actions"))
        .env_clear()
        .env("PATH", &fixture.path)
        .env("HOME", &fixture.home)
        .env("MISE_DATA_DIR", &fixture.mise_data_dir)
        .env("GH_TOKEN", "fixture_token")
        .env("MISE_NO_CONFIG", "1")
        .env("MISE_NO_ENV", "1")
        .env("MISE_NO_HOOKS", "1")
        .env("MISE_LOCKFILE", "0")
        .env("MISE_AUTO_INSTALL", "false")
        .env("MISE_EXEC_AUTO_INSTALL", "false")
        .env("VELNOR_INTERNAL_OP", "select-preview-source-v1")
        .env("RUNNER_TEMP", &fixture.runner_temp)
        .env("VELNOR_REQUEST_FILE", &fixture.policy_path)
        .env("VELNOR_RELEASE_POLICY", &fixture.policy_text)
        .env("VELNOR_RELEASE_STATUS_TOKEN", "fixture-status-read-token")
        .env("GITHUB_REPOSITORY", REPOSITORY)
        .env("GITHUB_REPOSITORY_ID", REPOSITORY_ID.to_string())
        .env("GITHUB_SHA", AUTHORITY_SHA)
        .env("GITHUB_WORKFLOW_SHA", AUTHORITY_SHA)
        .env("GITHUB_REF", "refs/heads/main")
        .env(
            "GITHUB_WORKFLOW_REF",
            format!("{REPOSITORY}/{PREVIEW_WORKFLOW_PATH}@refs/heads/main"),
        )
        .env("GITHUB_EVENT_NAME", "workflow_run")
        .env("GITHUB_EVENT_PATH", &fixture.event_path)
        .env("GITHUB_RUN_ID", PREVIEW_RUN_ID.to_string())
        .env("GITHUB_RUN_ATTEMPT", "1")
        .env("GITHUB_JOB", "preview-eligibility")
        .env("GITHUB_OUTPUT", &fixture.github_output)
        .output()?;
    let requests = fs::read_to_string(&fixture.request_log).unwrap_or_default();
    let gh_calls = requests.clone();
    Ok(Invocation {
        status: output.status,
        stdout: output.stdout,
        stderr: output.stderr,
        output: fs::read_to_string(&fixture.github_output)?,
        requests,
        gh_calls,
    })
}
