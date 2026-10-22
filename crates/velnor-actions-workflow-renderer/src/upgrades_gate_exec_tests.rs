//! Finding 5, executed: the freshness gate fails while a scope stays behind
//! after a rollback, even when prepare found nothing else to change.

use super::exec_support::{Sandbox, TestResult, text};

const GATE_ENV: [(&str, &str); 4] = [
    ("GH_TOKEN", "fixture-read-token"),
    ("VELNOR_UPGRADE_BRANCH", "velnor/upgrades"),
    ("VELNOR_STUCK_DAYS", "3"),
    ("VELNOR_PREPARE_RESULT", "success"),
];

fn gate(sandbox: &Sandbox, env: &[(&str, &str)]) -> TestResult<std::process::Output> {
    let mut all = GATE_ENV.to_vec();
    all.extend_from_slice(env);
    sandbox.run(super::GATE, &all)
}

#[test]
fn prepare_reports_a_rolled_back_scope_with_an_empty_diff() -> TestResult {
    // Every pin is current, but the Cargo scope cannot upgrade (a private
    // git source needs the fetch credential prepare never holds).
    let sandbox = Sandbox::new("rollback-empty")?;
    let output = sandbox.prepare(&[("SANDBOX_CARGO_UPGRADE_FAILS", "1")])?;
    let log = text(&output);
    assert!(output.status.success(), "{log}");
    let raw = std::fs::read_to_string(sandbox.root.join("github-output"))?;
    let outputs = sandbox.outputs();
    assert_eq!(
        outputs.get("changed").map(String::as_str),
        Some("false"),
        "{log}"
    );
    assert_eq!(
        outputs.get("failed").map(String::as_str),
        Some("true"),
        "{log}"
    );
    let (_, rest) = raw.split_once("failures<<").ok_or("no failures output")?;
    let (delimiter, body) = rest.split_once('\n').ok_or("no delimiter")?;
    let failures = body
        .split(&format!("\n{delimiter}\n"))
        .next()
        .unwrap_or_default();
    assert!(failures.contains("Cargo upgrade in . failed"), "{raw}");
    assert!(log.contains("## Needs migration"), "{log}");
    assert!(log.contains("Cargo upgrade in . failed"), "{log}");
    Ok(())
}

#[test]
fn prepare_reports_no_failure_when_every_scope_succeeds() -> TestResult {
    let sandbox = Sandbox::new("rollback-none")?;
    let output = sandbox.prepare(&[])?;
    let log = text(&output);
    assert!(output.status.success(), "{log}");
    let outputs = sandbox.outputs();
    assert_eq!(
        outputs.get("changed").map(String::as_str),
        Some("false"),
        "{log}"
    );
    assert_eq!(
        outputs.get("failed").map(String::as_str),
        Some("false"),
        "{log}"
    );
    Ok(())
}

#[test]
fn gate_fails_on_a_rolled_back_scope_even_without_a_diff() -> TestResult {
    let sandbox = Sandbox::new("gate-failed")?;
    let output = gate(
        &sandbox,
        &[
            ("VELNOR_STALE", "false"),
            ("VELNOR_SCOPE_FAILED", "true"),
            (
                "VELNOR_SCOPE_FAILURES",
                "- Cargo upgrade in . failed\n::add-mask::injected",
            ),
        ],
    )?;
    let log = text(&output);
    assert!(!output.status.success(), "{log}");
    assert!(
        log.contains("::error::upgrade_scope_needs_migration: "),
        "{log}"
    );
    assert!(log.contains("    - Cargo upgrade in . failed"), "{log}");
    assert!(
        !log.lines().any(|line| line.starts_with("::add-mask::")),
        "prepared text started a workflow command:\n{log}"
    );
    assert!(!log.contains("every pin is the latest"), "{log}");
    Ok(())
}

#[test]
fn gate_passes_only_when_nothing_lags_or_failed() -> TestResult {
    let sandbox = Sandbox::new("gate-green")?;
    let output = gate(
        &sandbox,
        &[("VELNOR_STALE", "false"), ("VELNOR_SCOPE_FAILED", "false")],
    )?;
    let log = text(&output);
    assert!(output.status.success(), "{log}");
    assert!(
        log.contains("every pin is the latest stable release"),
        "{log}"
    );
    Ok(())
}

#[test]
fn gate_reports_a_stuck_pull_request() -> TestResult {
    let sandbox = Sandbox::new("gate-stuck")?;
    sandbox.stub(
        "gh-impl",
        "#!/bin/sh\ncase \"$1 $2\" in \"pr list\") echo 2020-01-01T00:00:00Z ;; esac\n",
    )?;
    let output = gate(
        &sandbox,
        &[("VELNOR_STALE", "false"), ("VELNOR_SCOPE_FAILED", "false")],
    )?;
    let log = text(&output);
    assert!(!output.status.success(), "{log}");
    assert!(
        log.contains("::error::upgrade_pull_request_stuck: "),
        "{log}"
    );
    Ok(())
}
