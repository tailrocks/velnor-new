// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;
use std::io::Write;
use std::process::{Command, Stdio};

use serde_json::{Value, json};

const IDENTITY_SOURCE: &str = include_str!("../src/job_image_authority_identity.py");
const SOURCE: &str = "0123456789abcdef0123456789abcdef01234567";

const DRIVER: &str = r#"import json
import sys

sys.path.insert(0, sys.argv[1])
import job_image_authority_identity as identity_module

case = json.load(sys.stdin)
try:
    selected = identity_module.select_rust_ci_gate(
        case["runs"], case["jobs"], case["recheck"], case["source"]
    )
except identity_module.AuthorityError as error:
    print("REJECTED", error)
else:
    print("OK", selected["run_id"], selected["run_attempt"])
"#;

fn run(id: u64, number: u64, status: &str, conclusion: &Value) -> Value {
    json!({
        "id": id,
        "run_number": number,
        "run_attempt": 1,
        "path": ".github/workflows/rust-ci.yml",
        "event": "push",
        "head_branch": "main",
        "head_sha": SOURCE,
        "head_repository": {"full_name": "tailrocks/velnor-new"},
        "status": status,
        "conclusion": conclusion,
    })
}

fn green(id: u64, number: u64) -> Value {
    run(id, number, "completed", &json!("success"))
}

fn runs(entries: &[Value]) -> Value {
    json!({"workflow_runs": entries})
}

fn job(name: &str, run_id: u64, conclusion: &str) -> Value {
    json!({
        "name": name,
        "run_id": run_id,
        "run_attempt": 1,
        "head_sha": SOURCE,
        "head_branch": "main",
        "status": "completed",
        "conclusion": conclusion,
    })
}

fn jobs(entries: &[Value]) -> Value {
    json!([{"jobs": entries}])
}

fn decide(runs: &Value, jobs: &Value, recheck: &Value) -> Result<String, Box<dyn Error>> {
    let dir = tempfile::tempdir()?;
    std::fs::write(
        dir.path().join("job_image_authority_identity.py"),
        IDENTITY_SOURCE,
    )?;
    let mut child = Command::new("python3")
        .arg("-I")
        .arg("-c")
        .arg(DRIVER)
        .arg(dir.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    let case = json!({"runs": runs, "jobs": jobs, "recheck": recheck, "source": SOURCE});
    child
        .stdin
        .take()
        .ok_or("stdin")?
        .write_all(case.to_string().as_bytes())?;
    let output = child.wait_with_output()?;
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    Ok(String::from_utf8(output.stdout)?.trim().to_owned())
}

fn gate() -> String {
    crate::rust_ci_gate_golden::gate_job_name()
}

fn good_jobs() -> Value {
    jobs(&[
        job("rust-lint", 102, "success"),
        job(&gate(), 102, "success"),
    ])
}

#[test]
fn image_publication_needs_a_green_rust_required_job_on_the_same_source()
-> Result<(), Box<dyn Error>> {
    let latest = runs(&[green(102, 7)]);
    assert_eq!(decide(&latest, &good_jobs(), &latest)?, "OK 102 1");
    let older_and_newer = runs(&[green(90, 6), green(102, 7)]);
    assert_eq!(
        decide(&older_and_newer, &good_jobs(), &older_and_newer)?,
        "OK 102 1"
    );

    let pending = runs(&[run(102, 7, "in_progress", &Value::Null)]);
    let cancelled = runs(&[run(102, 7, "completed", &json!("cancelled"))]);
    let red = runs(&[run(102, 7, "completed", &json!("failure"))]);
    let tie = runs(&[green(102, 7), green(103, 7)]);
    let superseded_red = runs(&[green(101, 6), run(102, 7, "completed", &json!("failure"))]);
    for (label, listing, reason) in [
        ("missing", runs(&[]), "No canonical main Rust CI push run"),
        ("pending", pending, "no `success` conclusion"),
        ("cancelled", cancelled, "no `success` conclusion"),
        ("red", red, "no `success` conclusion"),
        ("run-number tie", tie, "no `success` conclusion"),
        (
            "older green, newest red",
            superseded_red,
            "no `success` conclusion",
        ),
    ] {
        let verdict = decide(&listing, &good_jobs(), &listing)?;
        assert!(
            verdict.starts_with("REJECTED") && verdict.contains(reason),
            "{label}: {verdict}"
        );
    }
    Ok(())
}

#[test]
fn image_publication_ignores_a_skipped_rust_required_job() -> Result<(), Box<dyn Error>> {
    let latest = runs(&[green(102, 7)]);
    for (label, entries) in [
        (
            "skipped last",
            vec![job(&gate(), 102, "success"), job(&gate(), 102, "skipped")],
        ),
        (
            "skipped first",
            vec![job(&gate(), 102, "skipped"), job(&gate(), 102, "success")],
        ),
    ] {
        let verdict = decide(&latest, &jobs(&entries), &latest)?;
        assert_eq!(verdict, "OK 102 1", "{label}");
    }
    for (label, entries, reason) in [
        (
            "only skipped",
            vec![job(&gate(), 102, "skipped")],
            "exactly one non-skipped `rust-required` job",
        ),
        (
            "two live",
            vec![job(&gate(), 102, "success"), job(&gate(), 102, "success")],
            "exactly one non-skipped `rust-required` job",
        ),
        (
            "live and red",
            vec![job(&gate(), 102, "success"), job(&gate(), 102, "failure")],
            "exactly one non-skipped `rust-required` job",
        ),
        (
            "red beside skipped",
            vec![job(&gate(), 102, "failure"), job(&gate(), 102, "skipped")],
            "has no `success` conclusion",
        ),
    ] {
        let verdict = decide(&latest, &jobs(&entries), &latest)?;
        assert!(
            verdict.starts_with("REJECTED") && verdict.contains(reason),
            "{label}: {verdict}"
        );
    }
    Ok(())
}

#[test]
fn image_publication_rejects_an_ambiguous_or_shifting_rust_required_job()
-> Result<(), Box<dyn Error>> {
    let latest = runs(&[green(102, 7)]);
    for (label, jobs, reason) in [
        (
            "duplicate",
            jobs(&[job(&gate(), 102, "success"), job(&gate(), 102, "success")]),
            "exactly one non-skipped `rust-required` job",
        ),
        (
            "missing",
            jobs(&[job("rust-lint", 102, "success")]),
            "exactly one non-skipped `rust-required` job",
        ),
        (
            "red",
            jobs(&[job(&gate(), 102, "failure")]),
            "has no `success` conclusion",
        ),
        (
            "other run",
            jobs(&[job(&gate(), 99, "success")]),
            "not on the selected Rust CI run",
        ),
        ("malformed", json!({"jobs": []}), "incorrect format"),
    ] {
        let verdict = decide(&latest, &jobs, &latest)?;
        assert!(
            verdict.starts_with("REJECTED") && verdict.contains(reason),
            "{label}: {verdict}"
        );
    }
    for (label, recheck) in [
        ("newer run", runs(&[green(102, 7), green(103, 8)])),
        (
            "restarted",
            runs(&[run(102, 7, "in_progress", &Value::Null)]),
        ),
        ("emptied", runs(&[])),
    ] {
        let verdict = decide(&latest, &good_jobs(), &recheck)?;
        assert!(
            verdict.starts_with("REJECTED"),
            "recheck {label}: {verdict}"
        );
    }
    let verdict = decide(
        &latest,
        &good_jobs(),
        &runs(&[green(102, 7), green(103, 8)]),
    )?;
    assert!(verdict.contains("changed during the check"), "{verdict}");
    Ok(())
}
