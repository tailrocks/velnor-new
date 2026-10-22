use std::error::Error;
use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

use super::{EMPTY_LISTING_JQ, RETRY_DELAYS_SECONDS, bash_function};

static NEXT_CASE: AtomicUsize = AtomicUsize::new(0);

const RUN: &str = r#"{"id":42,"run_number":7,"run_attempt":1}"#;

struct Outcome {
    success: bool,
    stdout: String,
    stderr: String,
    listings: usize,
    sleeps: Vec<String>,
}

/// Run the function against a lister that prints `responses[n]` on call `n`
/// (the last response repeats) and exits with `statuses[n]` (default 0).
fn run(responses: &[&str], statuses: &[i32]) -> Result<Outcome, Box<dyn Error>> {
    let directory = std::env::temp_dir().join(format!(
        "velnor-ci-run-lookup-{}-{}",
        std::process::id(),
        NEXT_CASE.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir(&directory)?;
    for (index, response) in responses.iter().enumerate() {
        fs::write(directory.join(format!("response-{}", index + 1)), response)?;
    }
    for (index, status) in statuses.iter().enumerate() {
        fs::write(
            directory.join(format!("status-{}", index + 1)),
            status.to_string(),
        )?;
    }
    let script = format!(
        r#"set -euo pipefail
sleep() {{ printf '%s\n' "$1" >> "$CASE/sleeps"; }}
lister() {{
  local count=0
  [[ ! -f "$CASE/count" ]] || count="$(cat "$CASE/count")"
  count=$((count + 1))
  printf '%s\n' "$count" > "$CASE/count"
  local response="$CASE/response-$count" status=0
  [[ -f "$response" ]] || response="$CASE/response-{last}"
  [[ ! -f "$CASE/status-$count" ]] || status="$(cat "$CASE/status-$count")"
  cat "$response"
  return "$status"
}}
{function}
list_exact_source_ci_runs 'lookup test' lister
"#,
        last = responses.len(),
        function = bash_function(),
    );
    let output = Command::new("bash")
        .arg("-c")
        .arg(script)
        .env("CASE", &directory)
        .output()?;
    let listings = read(&directory.join("count"))?.trim().parse::<usize>()?;
    let sleeps = read(&directory.join("sleeps"))?
        .lines()
        .map(str::to_owned)
        .collect();
    fs::remove_dir_all(&directory)?;
    Ok(Outcome {
        success: output.status.success(),
        stdout: String::from_utf8(output.stdout)?,
        stderr: String::from_utf8(output.stderr)?,
        listings,
        sleeps,
    })
}

fn read(path: &PathBuf) -> Result<String, Box<dyn Error>> {
    match fs::read_to_string(path) {
        Ok(text) => Ok(text),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(String::new()),
        Err(error) => Err(error.into()),
    }
}

fn page(runs: &[&str]) -> String {
    format!(
        r#"[{{"total_count":{},"workflow_runs":[{}]}}]"#,
        runs.len(),
        runs.join(",")
    )
}

#[test]
fn six_listings_within_ninety_seconds() {
    assert_eq!(RETRY_DELAYS_SECONDS.len() + 1, 6);
    assert_eq!(
        RETRY_DELAYS_SECONDS
            .iter()
            .map(|delay| u32::from(*delay))
            .sum::<u32>(),
        90
    );
    assert!(
        RETRY_DELAYS_SECONDS
            .windows(2)
            .all(|pair| pair[0] < pair[1])
    );
    let function = bash_function();
    assert!(function.contains("for _velnor_delay in 3 6 12 24 45 ''; do"));
    assert!(function.contains(EMPTY_LISTING_JQ));
    assert!(function.contains("stayed empty after 6 attempts"));
}

#[test]
fn a_listing_with_a_run_is_returned_without_waiting() -> Result<(), Box<dyn Error>> {
    let listing = page(&[RUN]);
    let outcome = run(&[&listing], &[])?;
    assert!(outcome.success, "{}", outcome.stderr);
    assert_eq!(outcome.stdout.trim(), listing);
    assert_eq!(outcome.listings, 1);
    assert!(outcome.sleeps.is_empty());
    Ok(())
}

#[test]
fn an_empty_listing_is_retried_until_a_run_appears() -> Result<(), Box<dyn Error>> {
    let listing = page(&[RUN]);
    let empty = page(&[]);
    let outcome = run(&[&empty, "[]", &listing], &[])?;
    assert!(outcome.success, "{}", outcome.stderr);
    assert_eq!(outcome.stdout.trim(), listing);
    assert_eq!(outcome.listings, 3);
    assert_eq!(outcome.sleeps, ["3", "6"]);
    assert_eq!(
        outcome
            .stderr
            .matches("lookup test: the CI run listing is empty; listing again in")
            .count(),
        2
    );
    Ok(())
}

#[test]
fn a_listing_that_stays_empty_ends_after_six_attempts() -> Result<(), Box<dyn Error>> {
    let empty = page(&[]);
    let outcome = run(&[&empty], &[])?;
    // The caller's identity checks turn the empty listing into a closed failure.
    assert!(outcome.success, "{}", outcome.stderr);
    assert_eq!(outcome.stdout.trim(), empty);
    assert_eq!(outcome.listings, 6);
    assert_eq!(outcome.sleeps, ["3", "6", "12", "24", "45"]);
    assert!(
        outcome
            .stderr
            .contains("lookup test: the CI run listing stayed empty after 6 attempts")
    );
    Ok(())
}

#[test]
fn an_empty_page_beside_a_run_is_not_empty() -> Result<(), Box<dyn Error>> {
    let listing = format!(
        r#"[{{"total_count":1,"workflow_runs":[]}},{{"total_count":1,"workflow_runs":[{RUN}]}}]"#
    );
    let outcome = run(&[&listing], &[])?;
    assert!(outcome.success, "{}", outcome.stderr);
    assert_eq!(outcome.listings, 1);
    assert!(outcome.sleeps.is_empty());
    Ok(())
}

#[test]
fn a_mismatching_run_is_final_and_never_retried() -> Result<(), Box<dyn Error>> {
    // Any run, even one every identity check will reject, makes the listing final.
    let wrong =
        r#"{"id":9,"run_number":1,"run_attempt":1,"event":"pull_request","head_branch":"topic"}"#;
    let listing = page(&[wrong]);
    let outcome = run(&[&listing, &page(&[RUN])], &[])?;
    assert!(outcome.success, "{}", outcome.stderr);
    assert_eq!(outcome.stdout.trim(), listing);
    assert_eq!(outcome.listings, 1);
    assert!(outcome.sleeps.is_empty());
    Ok(())
}

#[test]
fn a_failed_listing_fails_at_once() -> Result<(), Box<dyn Error>> {
    let outcome = run(&[&page(&[]), &page(&[RUN])], &[0, 1])?;
    assert!(!outcome.success);
    assert_eq!(outcome.listings, 2);
    assert_eq!(outcome.sleeps, ["3"]);
    let first_failure = run(&[&page(&[RUN])], &[22])?;
    assert!(!first_failure.success);
    assert_eq!(first_failure.listings, 1);
    assert!(first_failure.sleeps.is_empty());
    Ok(())
}

#[test]
fn output_that_is_not_a_listing_is_passed_on_unretried() -> Result<(), Box<dyn Error>> {
    for response in [
        "not json",
        r#"{"message":"Bad credentials"}"#,
        r#"[{"workflow_runs":3}]"#,
    ] {
        let outcome = run(&[response, &page(&[RUN])], &[])?;
        assert!(outcome.success, "{response}: {}", outcome.stderr);
        assert_eq!(outcome.stdout.trim(), response);
        assert_eq!(outcome.listings, 1, "{response}");
        assert!(outcome.sleeps.is_empty(), "{response}");
    }
    Ok(())
}

#[test]
fn null_workflow_runs_count_as_empty() -> Result<(), Box<dyn Error>> {
    let listing = page(&[RUN]);
    let outcome = run(
        &[r#"[{"total_count":0,"workflow_runs":null}]"#, &listing],
        &[],
    )?;
    assert!(outcome.success, "{}", outcome.stderr);
    assert_eq!(outcome.stdout.trim(), listing);
    assert_eq!(outcome.listings, 2);
    assert_eq!(outcome.sleeps, ["3"]);
    Ok(())
}
