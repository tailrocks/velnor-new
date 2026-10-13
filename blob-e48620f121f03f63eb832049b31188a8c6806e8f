use std::error::Error;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

use super::{ELIGIBILITY_SCRIPT, JOB_ID, REQUIRED_JOB_JQ, job, script};
use crate::schema2::product_release_test_pins::test_pins;

const SHA: &str = "0123456789abcdef0123456789abcdef01234567";
const AUTHORITY: &str = SHA;
const REPO: &str = "tailrocks/velnor-new";
const WORKFLOW: &str = "tailrocks/velnor-new/.github/workflows/product-release.yml@refs/heads/main";
static NEXT_CASE: AtomicUsize = AtomicUsize::new(0);

struct Scenario {
    repository: String,
    ref_name: String,
    event: String,
    workflow_ref: String,
    source_sha: String,
    authority_sha: String,
    main_sha: String,
    second_main_sha: Option<String>,
    runs: String,
    second_runs: Option<String>,
    jobs: String,
}

impl Scenario {
    fn valid() -> Self {
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
            event: "workflow_dispatch".to_owned(),
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

struct Execution {
    success: bool,
    stderr: String,
    output: String,
    calls: String,
}

struct Fixture {
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

const TIMEOUT_STUB: &str = r"#!/bin/sh
# Poison: the gh wrapper is a portable watchdog and must never invoke timeout.
echo 'poison: timeout must never be invoked' >&2
exit 99
";

const GIT_STUB: &str = r#"#!/bin/sh
set -eu
if [ "$1" = rev-parse ] && [ "$2" = HEAD ]; then printf '%s\n' "$GITHUB_SHA"; exit 0; fi
exec /usr/bin/git "$@"
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

fn write_executable(path: &Path, contents: &str) -> Result<(), Box<dyn Error>> {
    fs::write(path, contents)?;
    fs::set_permissions(path, fs::Permissions::from_mode(0o755))?;
    Ok(())
}

fn create_fixture(scenario: &Scenario) -> Result<Fixture, Box<dyn Error>> {
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
    write_executable(&bin.join("timeout"), TIMEOUT_STUB)?;
    write_executable(&bin.join("git"), GIT_STUB)?;
    Ok(Fixture {
        directory,
        bin,
        calls,
        output,
    })
}

fn run(
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

fn pages(pages: &[Vec<String>]) -> String {
    let encoded = pages
        .iter()
        .map(|runs| format!(r#"{{"workflow_runs":[{}]}}"#, runs.join(",")))
        .collect::<Vec<_>>()
        .join(",");
    format!("[{encoded}]")
}

fn required_job(
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

fn job_pages(jobs: &[String]) -> String {
    format!(r#"[{{"jobs":[{}]}}]"#, jobs.join(","))
}

fn execute(scenario: Scenario) -> Result<Execution, Box<dyn Error>> {
    let fixture = create_fixture(&scenario)?;
    let path = std::env::var("PATH")?;
    let output = Command::new("bash")
        .args(["-euo", "pipefail", "-c"])
        .arg(script(&test_pins())?)
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

#[test]
fn selects_latest_ci_attempt_across_paginated_results() -> Result<(), Box<dyn Error>> {
    let result = execute(Scenario::valid())?;
    assert!(result.success, "{}", result.stderr);
    assert!(result.output.contains(&format!("source_sha={SHA}")));
    assert!(
        result
            .output
            .contains(&format!("workflow_authority_sha={AUTHORITY}"))
    );
    assert!(result.output.contains("ci_run_id=42"));
    assert!(result.output.contains("ci_attempt=3"));
    assert!(result.calls.contains("/attempts/3/jobs?per_page=100"));
    Ok(())
}

#[test]
fn rejects_newer_failed_run_instead_of_reusing_old_success() -> Result<(), Box<dyn Error>> {
    let mut scenario = Scenario::valid();
    scenario.runs = pages(&[
        vec![run(
            41,
            11,
            1,
            SHA,
            "completed",
            "success",
            ".github/workflows/ci.yml",
        )],
        vec![run(
            42,
            12,
            1,
            SHA,
            "completed",
            "failure",
            ".github/workflows/ci.yml",
        )],
    ]);
    let result = execute(scenario)?;
    assert!(!result.success);
    assert!(
        result
            .stderr
            .contains("latest exact-source CI run did not succeed")
    );
    assert!(!result.calls.contains("/attempts/"));
    Ok(())
}

#[test]
fn script_uses_the_expected_paginated_selectors_and_bounded_wait() -> Result<(), Box<dyn Error>> {
    let rendered = script(&test_pins())?;
    assert!(!rendered.contains("@LATEST_RUN_JQ@"));
    assert!(!rendered.contains("@REQUIRED_JOB_JQ@"));
    assert!(rendered.contains("/actions/runs/$run_id/attempts/$run_attempt/jobs?per_page=100"));
    assert!(rendered.contains("--paginate --slurp"));
    assert!(rendered.contains("latest CI attempt changed during eligibility check"));
    assert!(rendered.contains("poll_limit=\"${VELNOR_RELEASE_CI_POLL_LIMIT:-240}\""));
    assert!(REQUIRED_JOB_JQ.contains(".name == \"Required\""));
    assert!(REQUIRED_JOB_JQ.contains(".run_attempt == $run_attempt"));
    assert!(rendered.contains("head_repository.full_name == $repository"));
    assert!(ELIGIBILITY_SCRIPT.contains("GITHUB_WORKFLOW_REF"));
    Ok(())
}

#[test]
fn job_renders_a_read_only_source_gate() {
    let (name, value) =
        job(crate::yaml::Yaml::str("ubuntu-latest"), &test_pins()).expect("pinned eligibility job");
    assert_eq!(name, JOB_ID);
    let rendered = crate::yaml::render_yaml(&crate::yaml::Yaml::Map(vec![(name, value)]));
    assert!(rendered.contains("actions: read"));
    assert!(rendered.contains("contents: read"));
    assert!(rendered.contains("gh@2.102.0"));
    assert!(rendered.contains("workflow_authority_sha"));
}

#[path = "schema2_release_eligibility_edge_tests.rs"]
mod edge;
