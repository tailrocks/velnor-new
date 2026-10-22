//! Executes the generator graph's exact-source CI check against stubbed APIs.
use super::{ci_check_script, write_executable};

use std::error::Error;
use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

const SHA: &str = "0123456789abcdef0123456789abcdef01234567";
const OTHER_SHA: &str = "fedcba9876543210fedcba9876543210fedcba98";
const REPOSITORY: &str = "tailrocks/velnor-new";
static NEXT_CASE: AtomicUsize = AtomicUsize::new(0);

const GH_STUB: &str = r#"#!/bin/sh
set -eu
endpoint=
for arg do
  case "$arg" in repos/*) [ -n "$endpoint" ] || endpoint="$arg" ;; esac
done
printf '%s\n' "$endpoint" >> "$CASE/calls"
case "$endpoint" in
  *"/actions/workflows/ci.yml/runs?"*)
    count=0
    [ ! -f "$CASE/runs-count" ] || IFS= read -r count < "$CASE/runs-count"
    count=$((count + 1))
    printf '%s\n' "$count" > "$CASE/runs-count"
    [ ! -f "$CASE/runs-$count.fail" ] || { echo 'HTTP 502' >&2; exit 1; }
    response="$CASE/runs-$count.json"
    [ -f "$response" ] || response="$CASE/runs.json"
    cat "$response"
    ;;
  *"/attempts/"*"/jobs?per_page=100") cat "$CASE/jobs.json" ;;
  *"/commits/main") printf '%s\n' "$GITHUB_SHA" ;;
  *) printf 'unexpected endpoint: %s\n' "$endpoint" >&2; exit 64 ;;
esac
"#;

const GIT_STUB: &str = r#"#!/bin/sh
set -eu
[ "$1" = rev-parse ] && [ "$2" = HEAD ] && { printf '%s\n' "$GITHUB_SHA"; exit 0; }
exit 64
"#;

const SLEEP_STUB: &str = r#"#!/bin/sh
case "$1" in
  3|6|12|24|45) printf '%s\n' "$1" >> "$CASE/sleeps"; exit 0 ;;
esac
exec /bin/sleep "$@"
"#;

struct Outcome {
    success: bool,
    stderr: String,
    listings: usize,
    sleeps: String,
    jobs_checked: bool,
}

fn run_json(sha: &str, event: &str, branch: &str) -> String {
    format!(
        r#"{{"id":42,"run_number":12,"run_attempt":3,"head_repository":{{"full_name":"{REPOSITORY}"}},"head_sha":"{sha}","head_branch":"{branch}","event":"{event}","status":"completed","conclusion":"success","path":".github/workflows/ci.yml"}}"#
    )
}

fn listing(runs: &[String]) -> String {
    format!(
        r#"[{{"total_count":{},"workflow_runs":[{}]}}]"#,
        runs.len(),
        runs.join(",")
    )
}

/// `by_call` overrides listing call `n` with a body, or with `None` to fail it.
fn execute(default: &str, by_call: &[(usize, Option<String>)]) -> Result<Outcome, Box<dyn Error>> {
    let case: PathBuf = std::env::temp_dir().join(format!(
        "velnor-generator-ci-check-{}-{}",
        std::process::id(),
        NEXT_CASE.fetch_add(1, Ordering::Relaxed)
    ));
    let bin = case.join("bin");
    fs::create_dir_all(&bin)?;
    write_executable(&bin.join("gh"), GH_STUB)?;
    write_executable(&bin.join("git"), GIT_STUB)?;
    write_executable(&bin.join("sleep"), SLEEP_STUB)?;
    fs::write(case.join("runs.json"), default)?;
    for (call, body) in by_call {
        match body {
            Some(body) => fs::write(case.join(format!("runs-{call}.json")), body)?,
            None => fs::write(case.join(format!("runs-{call}.fail")), "")?,
        }
    }
    fs::write(
        case.join("jobs.json"),
        format!(
            r#"[{{"jobs":[{{"name":"Required","head_sha":"{SHA}","head_branch":"main","status":"completed","conclusion":"success"}}]}}]"#
        ),
    )?;
    fs::write(case.join("calls"), "")?;
    fs::write(case.join("sleeps"), "")?;
    let path = std::env::var("PATH")?;
    let output = Command::new("bash")
        .args(["-c", &ci_check_script()])
        .env("PATH", format!("{}:{path}", bin.display()))
        .env("CASE", &case)
        .env("GITHUB_REPOSITORY", REPOSITORY)
        .env("GITHUB_REF", "refs/heads/main")
        .env("GITHUB_EVENT_NAME", "workflow_dispatch")
        .env("GITHUB_SHA", SHA)
        .env("GITHUB_WORKFLOW_SHA", SHA)
        .output()?;
    let calls = fs::read_to_string(case.join("calls"))?;
    let outcome = Outcome {
        success: output.status.success(),
        stderr: String::from_utf8(output.stderr)?,
        listings: calls
            .lines()
            .filter(|line| line.contains("/actions/workflows/ci.yml/runs?"))
            .count(),
        sleeps: fs::read_to_string(case.join("sleeps"))?,
        jobs_checked: calls.contains("/attempts/3/jobs?per_page=100"),
    };
    fs::remove_dir_all(&case)?;
    Ok(outcome)
}

#[test]
fn lists_runs_by_head_sha_and_keeps_identity_in_jq() {
    let script = ci_check_script();
    assert!(script.contains(
        "list_exact_source_ci_runs 'verify release source' gh api --paginate --slurp \"repos/tailrocks/velnor-new/actions/workflows/ci.yml/runs?head_sha=$GITHUB_SHA&per_page=100\""
    ));
    assert!(!script.contains("&branch=main"));
    assert!(!script.contains("&event=push"));
    for check in [
        r#".path == ".github/workflows/ci.yml""#,
        ".head_sha == $sha",
        r#".head_branch == "main""#,
        ".head_repository.full_name == $repo",
        r#".event == "push""#,
    ] {
        assert!(script.contains(check), "{check}");
    }
}

#[test]
fn accepts_the_exact_source_run() -> Result<(), Box<dyn Error>> {
    let outcome = execute(&listing(&[run_json(SHA, "push", "main")]), &[])?;
    assert!(outcome.success, "{}", outcome.stderr);
    assert_eq!(outcome.listings, 1);
    assert_eq!(outcome.sleeps, "");
    assert!(outcome.jobs_checked);
    Ok(())
}

#[test]
fn retries_only_an_empty_listing() -> Result<(), Box<dyn Error>> {
    let valid = listing(&[run_json(SHA, "push", "main")]);
    let outcome = execute(
        &valid,
        &[(1, Some(listing(&[]))), (2, Some("[]".to_owned()))],
    )?;
    assert!(outcome.success, "{}", outcome.stderr);
    assert_eq!(outcome.listings, 3);
    assert_eq!(outcome.sleeps, "3\n6\n");
    assert!(outcome.jobs_checked);
    Ok(())
}

#[test]
fn fails_closed_when_the_listing_stays_empty() -> Result<(), Box<dyn Error>> {
    let outcome = execute(&listing(&[]), &[])?;
    assert!(!outcome.success);
    assert!(
        outcome.stderr.contains("no exact-source main CI run"),
        "{}",
        outcome.stderr
    );
    assert!(outcome.stderr.contains("stayed empty after 6 attempts"));
    assert_eq!(outcome.listings, 6);
    assert_eq!(outcome.sleeps, "3\n6\n12\n24\n45\n");
    assert!(!outcome.jobs_checked);
    Ok(())
}

#[test]
fn never_retries_a_mismatch_or_a_failed_listing() -> Result<(), Box<dyn Error>> {
    let valid = listing(&[run_json(SHA, "push", "main")]);
    for mismatch in [
        run_json(OTHER_SHA, "push", "main"),
        run_json(SHA, "workflow_dispatch", "main"),
        run_json(SHA, "push", "topic"),
        run_json(SHA, "push", "main").replace(REPOSITORY, "fork/velnor-new"),
    ] {
        let outcome = execute(&valid, &[(1, Some(listing(&[mismatch.clone()])))])?;
        assert!(!outcome.success, "{mismatch}");
        assert!(
            outcome.stderr.contains("no exact-source main CI run"),
            "{mismatch}"
        );
        assert_eq!(outcome.listings, 1, "{mismatch}");
        assert_eq!(outcome.sleeps, "", "{mismatch}");
    }
    let failed = execute(&valid, &[(1, None)])?;
    assert!(!failed.success);
    assert_eq!(failed.listings, 1);
    assert_eq!(failed.sleeps, "");
    assert!(!failed.jobs_checked);
    Ok(())
}
