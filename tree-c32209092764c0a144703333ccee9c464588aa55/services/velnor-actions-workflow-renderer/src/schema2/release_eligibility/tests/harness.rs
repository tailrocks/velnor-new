use super::super::script;
use super::{AUTHORITY, REPO, SHA, WORKFLOW};
use std::error::Error;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

static NEXT_CASE: AtomicUsize = AtomicUsize::new(0);

pub(super) struct Scenario {
    pub(super) repository: String,
    pub(super) ref_name: String,
    pub(super) event: String,
    pub(super) workflow_ref: String,
    pub(super) source_sha: String,
    pub(super) authority_sha: String,
    pub(super) main_sha: String,
    pub(super) second_main_sha: Option<String>,
    pub(super) runs: String,
    pub(super) second_runs: Option<String>,
    pub(super) jobs: String,
}

impl Scenario {
    pub(super) fn valid() -> Self {
        let newest = run(
            42,
            12,
            3,
            SHA,
            "completed",
            "success",
            ".github/workflows/ci.yml",
        );
        Self {
            repository: REPO.to_owned(),
            ref_name: "refs/heads/main".to_owned(),
            event: "push".to_owned(),
            workflow_ref: WORKFLOW.to_owned(),
            source_sha: SHA.to_owned(),
            authority_sha: AUTHORITY.to_owned(),
            main_sha: SHA.to_owned(),
            second_main_sha: None,
            runs: pages(&[
                vec![run(
                    41,
                    11,
                    1,
                    SHA,
                    "completed",
                    "success",
                    ".github/workflows/ci.yml",
                )],
                vec![newest],
            ]),
            second_runs: None,
            jobs: job_pages(&[required_job(42, 3, SHA, "main", "completed", "success")]),
        }
    }
}

pub(super) struct Execution {
    pub(super) success: bool,
    pub(super) stderr: String,
    pub(super) output: String,
    pub(super) calls: String,
}

pub(super) struct Fixture {
    directory: PathBuf,
    bin: PathBuf,
    calls: PathBuf,
    output: PathBuf,
}

const MISE_STUB: &str = r#"#!/bin/sh
set -eu
[ "$1" = "--no-config" ] && shift
[ "$1" = "--no-env" ] && shift
[ "$1" = "--no-hooks" ] && shift
[ "$1" = "exec" ] && shift
[ "$1" = "gh@2.102.0" ] && shift
[ "$1" = "--" ] && shift
[ "$1" = "gh" ] && shift
exec gh "$@"
"#;

const GH_STUB: &str = r#"#!/bin/sh
set -eu
endpoint=
for arg do endpoint="$arg"; done
printf '%s\n' "$endpoint" >> "$VELNOR_TEST_CALLS"
case "$endpoint" in
  *"/commits/main")
    counter="$VELNOR_TEST_FIXTURES/main-count"
    count=0
    [ ! -f "$counter" ] || IFS= read -r count < "$counter"
    count=$((count + 1))
    printf '%s\n' "$count" > "$counter"
    response="$VELNOR_TEST_FIXTURES/main-$count.json"
    [ -f "$response" ] || response="$VELNOR_TEST_FIXTURES/main.json"
    cat "$response"
    ;;
  *"/actions/workflows/ci.yml/runs?"*)
    counter="$VELNOR_TEST_FIXTURES/runs-count"
    count=0
    [ ! -f "$counter" ] || IFS= read -r count < "$counter"
    count=$((count + 1))
    printf '%s\n' "$count" > "$counter"
    response="$VELNOR_TEST_FIXTURES/runs-$count.json"
    [ -f "$response" ] || response="$VELNOR_TEST_FIXTURES/runs.json"
    cat "$response"
    ;;
  *"/attempts/"*"/jobs?per_page=100") cat "$VELNOR_TEST_FIXTURES/jobs.json" ;;
  *) printf 'unexpected endpoint: %s\n' "$endpoint" >&2; exit 64 ;;
esac
"#;

pub(super) fn write_executable(path: &Path, contents: &str) -> Result<(), Box<dyn Error>> {
    fs::write(path, contents)?;
    fs::set_permissions(path, fs::Permissions::from_mode(0o755))?;
    Ok(())
}

pub(super) fn create_fixture(scenario: &Scenario) -> Result<Fixture, Box<dyn Error>> {
    let id = NEXT_CASE.fetch_add(1, Ordering::Relaxed);
    let directory =
        std::env::temp_dir().join(format!("velnor-release-gate-{}-{id}", std::process::id()));
    let bin = directory.join("bin");
    fs::create_dir_all(&bin)?;
    fs::write(
        directory.join("main.json"),
        format!(r#"{{"sha":"{}"}}"#, scenario.main_sha),
    )?;
    if let Some(sha) = &scenario.second_main_sha {
        fs::write(
            directory.join("main-2.json"),
            format!(r#"{{"sha":"{sha}"}}"#),
        )?;
    }
    fs::write(directory.join("runs.json"), &scenario.runs)?;
    if let Some(runs) = &scenario.second_runs {
        fs::write(directory.join("runs-2.json"), runs)?;
    }
    fs::write(directory.join("jobs.json"), &scenario.jobs)?;
    let calls = directory.join("calls");
    let output = directory.join("outputs");
    fs::write(&calls, "")?;
    fs::write(&output, "")?;
    write_executable(&bin.join("mise"), MISE_STUB)?;
    write_executable(&bin.join("gh"), GH_STUB)?;
    Ok(Fixture {
        directory,
        bin,
        calls,
        output,
    })
}

pub(super) fn run(
    id: u64,
    number: u64,
    attempt: u64,
    sha: &str,
    status: &str,
    conclusion: &str,
    path: &str,
) -> String {
    format!(
        r#"{{"id":{id},"run_number":{number},"run_attempt":{attempt},"head_repository":{{"full_name":"{REPO}"}},"head_sha":"{sha}","head_branch":"main","event":"push","status":"{status}","conclusion":"{conclusion}","path":"{path}"}}"#
    )
}

pub(super) fn pages(pages: &[Vec<String>]) -> String {
    let encoded = pages
        .iter()
        .map(|runs| format!(r#"{{"workflow_runs":[{}]}}"#, runs.join(",")))
        .collect::<Vec<_>>()
        .join(",");
    format!("[{encoded}]")
}

pub(super) fn required_job(
    run_id: u64,
    attempt: u64,
    sha: &str,
    branch: &str,
    status: &str,
    conclusion: &str,
) -> String {
    format!(
        r#"{{"id":300,"run_id":{run_id},"run_attempt":{attempt},"head_sha":"{sha}","head_branch":"{branch}","name":"Required","status":"{status}","conclusion":"{conclusion}"}}"#
    )
}

pub(super) fn job_pages(jobs: &[String]) -> String {
    format!(r#"[{{"jobs":[{}]}}]"#, jobs.join(","))
}

pub(super) fn execute(scenario: Scenario) -> Result<Execution, Box<dyn Error>> {
    let fixture = create_fixture(&scenario)?;
    let path = std::env::var("PATH")?;
    let output = Command::new("bash")
        .args(["-euo", "pipefail", "-c"])
        .arg(script())
        .env("PATH", format!("{}:{path}", fixture.bin.display()))
        .env("VELNOR_TEST_FIXTURES", &fixture.directory)
        .env("VELNOR_TEST_CALLS", &fixture.calls)
        .env("VELNOR_RELEASE_CI_POLL_LIMIT", "1")
        .env("VELNOR_RELEASE_CI_POLL_SECONDS", "0")
        .env("GITHUB_REPOSITORY", scenario.repository)
        .env("GITHUB_REF", scenario.ref_name)
        .env("GITHUB_EVENT_NAME", scenario.event)
        .env("GITHUB_WORKFLOW_REF", scenario.workflow_ref)
        .env("GITHUB_SHA", scenario.source_sha)
        .env("GITHUB_WORKFLOW_SHA", scenario.authority_sha)
        .env("GITHUB_OUTPUT", &fixture.output)
        .env("GH_TOKEN", "fixture-token")
        .output()?;
    let execution = Execution {
        success: output.status.success(),
        stderr: String::from_utf8(output.stderr)?,
        output: fs::read_to_string(&fixture.output)?,
        calls: fs::read_to_string(&fixture.calls)?,
    };
    fs::remove_dir_all(&fixture.directory)?;
    Ok(execution)
}
