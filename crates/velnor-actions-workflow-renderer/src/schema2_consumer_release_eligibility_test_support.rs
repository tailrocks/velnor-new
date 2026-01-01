use std::error::Error;
use std::fs;
use std::io::{Error as IoError, ErrorKind};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicUsize, Ordering};

use super::super::{ReleaseEligibilityContext, consumer_script};
use crate::schema2::git_fixture;

pub(super) const REPOSITORY: &str = "example/repo-scan";
pub(super) const DEFAULT_BRANCH: &str = "stable";
pub(super) const SOURCE_TOKEN: &str = "__VELNOR_TEST_SOURCE_SHA__";
pub(super) const OTHER_SHA: &str = "fedcba9876543210fedcba9876543210fedcba98";
static NEXT_CASE: AtomicUsize = AtomicUsize::new(0);

pub(super) fn context() -> ReleaseEligibilityContext {
    ReleaseEligibilityContext {
        repository: REPOSITORY.to_owned(),
        default_branch: DEFAULT_BRANCH.to_owned(),
        workflow_path: ".github/workflows/binary-release.yml".to_owned(),
    }
}

pub(super) struct Scenario {
    pub(super) repository: String,
    pub(super) ref_name: String,
    pub(super) event: String,
    pub(super) workflow_ref: String,
    pub(super) source_sha: Option<String>,
    pub(super) authority_sha: Option<String>,
    pub(super) default_branch_response: String,
    pub(super) tip: String,
    pub(super) second_tip: Option<String>,
    pub(super) runs: String,
    pub(super) second_runs: Option<String>,
    pub(super) jobs: String,
}

impl Scenario {
    pub(super) fn valid() -> Self {
        Self {
            repository: REPOSITORY.to_owned(),
            ref_name: format!("refs/heads/{DEFAULT_BRANCH}"),
            event: "workflow_dispatch".to_owned(),
            workflow_ref: format!(
                "{REPOSITORY}/.github/workflows/binary-release.yml@refs/heads/{DEFAULT_BRANCH}"
            ),
            source_sha: None,
            authority_sha: None,
            default_branch_response: format!(r#"{{"default_branch":"{DEFAULT_BRANCH}"}}"#),
            tip: format!(r#"[{{"sha":"{SOURCE_TOKEN}"}}]"#),
            second_tip: None,
            runs: pages(&[
                vec![ci_run(
                    41,
                    11,
                    1,
                    SOURCE_TOKEN,
                    DEFAULT_BRANCH,
                    "push",
                    ("completed", "success"),
                )],
                vec![ci_run(
                    42,
                    12,
                    3,
                    SOURCE_TOKEN,
                    DEFAULT_BRANCH,
                    "push",
                    ("completed", "success"),
                )],
            ]),
            second_runs: None,
            jobs: job_pages(&[
                vec![r#"{"id":301,"name":"Build"}"#.to_owned()],
                vec![required_job(
                    42,
                    3,
                    SOURCE_TOKEN,
                    DEFAULT_BRANCH,
                    "completed",
                    "success",
                )],
            ]),
        }
    }
}

pub(super) struct Execution {
    pub(super) success: bool,
    pub(super) stderr: String,
    pub(super) output: String,
    pub(super) calls: String,
}

struct Fixture {
    directory: PathBuf,
    repository: PathBuf,
    bin: PathBuf,
    calls: PathBuf,
    output: PathBuf,
}

impl Fixture {
    fn create() -> Result<Self, Box<dyn Error>> {
        let directory = create_temp_directory()?;
        Ok(Self {
            repository: directory.join("repo"),
            bin: directory.join("bin"),
            calls: directory.join("calls"),
            output: directory.join("outputs"),
            directory,
        })
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        drop(fs::remove_dir_all(&self.directory));
    }
}

const GH_STUB: &str = r#"#!/bin/sh
set -eu
endpoint=
paginate=false
slurp=false
for arg do
  case "$arg" in
    repos/*) [ -n "$endpoint" ] || endpoint="$arg" ;;
    --paginate) paginate=true ;;
    --slurp) slurp=true ;;
  esac
done
[ -n "$endpoint" ] || { echo 'missing API endpoint' >&2; exit 64; }
printf '%s\n' "$endpoint" >> "$VELNOR_TEST_CALLS"
need_pages() { [ "$paginate" = true ] && [ "$slurp" = true ] || { echo 'expected --paginate and --slurp' >&2; exit 64; }; }
case "$endpoint" in
  "repos/$VELNOR_TEST_REPOSITORY") cat "$VELNOR_TEST_FIXTURES/default-branch.json" ;;
  "repos/$VELNOR_TEST_REPOSITORY/commits")
    counter="$VELNOR_TEST_FIXTURES/tip-count"
    count=0
    [ ! -f "$counter" ] || IFS= read -r count < "$counter"
    count=$((count + 1))
    printf '%s\n' "$count" > "$counter"
    response="$VELNOR_TEST_FIXTURES/tip-$count.json"
    [ -f "$response" ] || response="$VELNOR_TEST_FIXTURES/tip.json"
    cat "$response"
    ;;
  "repos/$VELNOR_TEST_REPOSITORY/actions/workflows/ci.yml/runs")
    need_pages
    counter="$VELNOR_TEST_FIXTURES/runs-count"
    count=0
    [ ! -f "$counter" ] || IFS= read -r count < "$counter"
    count=$((count + 1))
    printf '%s\n' "$count" > "$counter"
    response="$VELNOR_TEST_FIXTURES/runs-$count.json"
    [ -f "$response" ] || response="$VELNOR_TEST_FIXTURES/runs.json"
    cat "$response"
    ;;
  "repos/$VELNOR_TEST_REPOSITORY/actions/runs/"*"/attempts/"*"/jobs?per_page=100")
    need_pages
    cat "$VELNOR_TEST_FIXTURES/jobs.json"
    ;;
  *) printf 'unexpected endpoint: %s\n' "$endpoint" >&2; exit 64 ;;
esac
"#;

fn create_temp_directory() -> Result<PathBuf, Box<dyn Error>> {
    let base = std::env::temp_dir();
    for _ in 0..128 {
        let id = NEXT_CASE.fetch_add(1, Ordering::Relaxed);
        let path = base.join(format!(
            "velnor-consumer-release-{}-{id}",
            std::process::id()
        ));
        match fs::create_dir(&path) {
            Ok(()) => return Ok(path),
            Err(error) if error.kind() == ErrorKind::AlreadyExists => {}
            Err(error) => return Err(error.into()),
        }
    }
    Err(IoError::new(
        ErrorKind::AlreadyExists,
        "temporary path collision limit exceeded",
    )
    .into())
}

fn checked_git(
    repository: &Path,
    args: &[&str],
    fixed_commit_time: bool,
) -> Result<Output, Box<dyn Error>> {
    let mut command = git_fixture::command(repository)?;
    command.args(args);
    if fixed_commit_time {
        command
            .env("GIT_AUTHOR_DATE", "2000-01-01T00:00:00+00:00")
            .env("GIT_COMMITTER_DATE", "2000-01-01T00:00:00+00:00");
    }
    let output = command.output()?;
    if !output.status.success() {
        return Err(format!(
            "git {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr)
        )
        .into());
    }
    Ok(output)
}

fn create_git_repo(repository: &Path) -> Result<String, Box<dyn Error>> {
    checked_git(repository, &["init", "--quiet"], false)?;
    fs::write(
        repository.join("tracked.txt"),
        "consumer release eligibility fixture\n",
    )?;
    checked_git(repository, &["add", "tracked.txt"], false)?;
    checked_git(
        repository,
        &[
            "-c",
            "user.name=Velnor Test",
            "-c",
            "user.email=velnor-test@example.invalid",
            "-c",
            "commit.gpgsign=false",
            "commit",
            "--quiet",
            "-m",
            "fixture",
        ],
        true,
    )?;
    let head = checked_git(repository, &["rev-parse", "HEAD"], false)?;
    Ok(String::from_utf8(head.stdout)?.trim().to_owned())
}

fn materialize(value: &str, source_sha: &str) -> String {
    value.replace(SOURCE_TOKEN, source_sha)
}

fn write_fixtures(
    fixture: &Fixture,
    scenario: &Scenario,
    source_sha: &str,
) -> Result<(), Box<dyn Error>> {
    fs::write(
        fixture.directory.join("default-branch.json"),
        materialize(&scenario.default_branch_response, source_sha),
    )?;
    fs::write(
        fixture.directory.join("tip.json"),
        materialize(&scenario.tip, source_sha),
    )?;
    if let Some(tip) = &scenario.second_tip {
        fs::write(
            fixture.directory.join("tip-2.json"),
            materialize(tip, source_sha),
        )?;
    }
    fs::write(
        fixture.directory.join("runs.json"),
        materialize(&scenario.runs, source_sha),
    )?;
    if let Some(runs) = &scenario.second_runs {
        fs::write(
            fixture.directory.join("runs-2.json"),
            materialize(runs, source_sha),
        )?;
    }
    fs::write(
        fixture.directory.join("jobs.json"),
        materialize(&scenario.jobs, source_sha),
    )?;
    fs::write(&fixture.calls, "")?;
    fs::write(&fixture.output, "")?;
    write_executable(&fixture.bin.join("gh"), GH_STUB)
}

fn create_fixture(scenario: &Scenario) -> Result<(Fixture, String), Box<dyn Error>> {
    let fixture = Fixture::create()?;
    fs::create_dir(&fixture.repository)?;
    fs::create_dir(&fixture.bin)?;
    let source_sha = create_git_repo(&fixture.repository)?;
    write_fixtures(&fixture, scenario, &source_sha)?;
    Ok((fixture, source_sha))
}

fn write_executable(path: &Path, contents: &str) -> Result<(), Box<dyn Error>> {
    fs::write(path, contents)?;
    fs::set_permissions(path, fs::Permissions::from_mode(0o755))?;
    Ok(())
}

pub(super) fn ci_run(
    id: u64,
    run_number: u64,
    attempt: u64,
    sha: &str,
    branch: &str,
    event: &str,
    outcome: (&str, &str),
) -> String {
    let (status, conclusion) = outcome;
    format!(
        r#"{{"id":{id},"run_number":{run_number},"run_attempt":{attempt},"head_repository":{{"full_name":"{REPOSITORY}"}},"head_sha":"{sha}","head_branch":"{branch}","event":"{event}","status":"{status}","conclusion":"{conclusion}","path":".github/workflows/ci.yml"}}"#
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

pub(super) fn job_pages(pages: &[Vec<String>]) -> String {
    let encoded = pages
        .iter()
        .map(|jobs| format!(r#"{{"jobs":[{}]}}"#, jobs.join(",")))
        .collect::<Vec<_>>()
        .join(",");
    format!("[{encoded}]")
}

pub(super) fn execute(scenario: &Scenario) -> Result<Execution, Box<dyn Error>> {
    let (fixture, git_sha) = create_fixture(scenario)?;
    let path = std::env::var("PATH")?;
    let source_sha = scenario.source_sha.as_deref().unwrap_or(&git_sha);
    let authority_sha = scenario.authority_sha.as_deref().unwrap_or(source_sha);
    let command_output = Command::new("bash")
        .args(["-euo", "pipefail", "-c"])
        .arg(consumer_script(
            &context(),
            &["/usr/bin/env".to_owned(), "gh".to_owned()],
            false,
        )?)
        .current_dir(&fixture.repository)
        .env("PATH", format!("{}:{path}", fixture.bin.display()))
        .env("VELNOR_TEST_FIXTURES", &fixture.directory)
        .env("VELNOR_TEST_CALLS", &fixture.calls)
        .env("VELNOR_TEST_REPOSITORY", REPOSITORY)
        .env("VELNOR_RELEASE_CI_POLL_LIMIT", "2")
        .env("VELNOR_RELEASE_CI_POLL_SECONDS", "0")
        .env("GITHUB_REPOSITORY", &scenario.repository)
        .env("GITHUB_REF", &scenario.ref_name)
        .env("GITHUB_EVENT_NAME", &scenario.event)
        .env("GITHUB_WORKFLOW_REF", &scenario.workflow_ref)
        .env("GITHUB_SHA", source_sha)
        .env("GITHUB_WORKFLOW_SHA", authority_sha)
        .env("GITHUB_OUTPUT", &fixture.output)
        .env("GH_TOKEN", "consumer-test-token")
        .output()?;
    Ok(Execution {
        success: command_output.status.success(),
        stderr: String::from_utf8(command_output.stderr)?,
        output: fs::read_to_string(&fixture.output)?,
        calls: fs::read_to_string(&fixture.calls)?,
    })
}

pub(super) fn assert_rejected(result: &Execution) {
    assert!(
        !result.success,
        "unexpected success; output: {}",
        result.output
    );
    assert!(
        result.output.is_empty(),
        "rejected gate wrote outputs: {}",
        result.output
    );
}
