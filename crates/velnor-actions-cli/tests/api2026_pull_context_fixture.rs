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
    Valid,
    ReversedParents,
    ChangedRef,
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
    let digest = sha256(&fs::read(cli)?);
    let git = git::Fixture::new(&paths);
    let identities = git.initialize(&digest)?;
    write_event(&paths, &identities)?;
    provider::write(&paths, &identities, scenario)?;
    let output = invoke(&paths, &identities)?;
    assert!(!output.status.success(), "fixture unexpectedly passed");
    let stderr = String::from_utf8_lossy(&output.stderr);
    let requests = fs::read_to_string(paths.root.join("provider/requests.log"))?;
    assert!(
        stderr.contains(expected),
        "expected {expected}, got {stderr}; fixture routes: {requests}"
    );
    assert!(!paths.runner_temp.join("velnor/local/plan.json").exists());
    assert!(
        !paths
            .runner_temp
            .join("velnor/request/plan-v1-response.json")
            .exists()
    );
    Ok(())
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

fn invoke(paths: &Paths, ids: &Identities) -> Result<Output, Box<dyn Error>> {
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
    let output = Command::new(cli)
        .current_dir(&paths.repo)
        .env_clear()
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

fn sha256(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut text = String::with_capacity(64);
    for byte in Sha256::digest(bytes) {
        text.push(char::from(HEX[usize::from(byte >> 4)]));
        text.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    text
}
