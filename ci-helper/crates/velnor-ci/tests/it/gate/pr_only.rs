// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use clap::Parser;
use serde_json::{Value, json};
use velnor_ci::cli::{Cli, Command};

use super::{GREEN, Recorded, assert_refused, check, required};

const ABSENT: &str = "no job of a push run on main reports it";

fn dco() -> Value {
    json!({"context": "DCO", "integration_id": 343_649})
}

fn protected() -> Recorded {
    Recorded::new().rules(vec![
        json!({"type": "pull_request"}),
        required(&[check("Required"), dco()]),
    ])
}

fn allowed(recorded: &Recorded) -> Result<velnor_ci::gate::Verdict, String> {
    recorded.gate_with(GREEN, &["DCO".to_owned()])
}

#[test]
fn an_explicit_pr_only_required_external_check_needs_no_push_job() {
    let verdict = allowed(&protected()).unwrap_or_else(|e| unreachable!("{e}"));
    assert!(verdict.ready);
    assert!(verdict.report.contains("check DCO is explicitly PR-only\n"));
    assert!(verdict.report.contains("check Required passed\n"));
    assert_refused(&protected().gate(GREEN), "GitHub App 343649");
    let missing = protected().requiring(&[check("Required"), check("DCO")]);
    assert_refused(&missing.gate(GREEN), ABSENT);
}

#[test]
fn pr_only_exemptions_require_live_required_names_and_a_pull_request_rule() {
    let stale = protected().rules(vec![
        json!({"type": "pull_request"}),
        required(&[check("Required")]),
    ]);
    assert_refused(&allowed(&stale), "PR-only check DCO is not required");
    let typo = protected().gate_with(GREEN, &["DCO typo".to_owned()]);
    assert_refused(&typo, "PR-only check DCO typo is not required");
    let rules = protected().requiring(&[check("Required"), dco()]);
    let classic =
        protected()
            .rules(vec![])
            .protection("main", "non_admins", &["Required"], &json!([dco()]));
    for recorded in [rules, classic] {
        assert_refused(&allowed(&recorded), "branch main has no pull_request rule");
    }
}

#[test]
fn pr_only_checks_keep_other_checks_bound_to_successful_exact_sha_push_jobs() {
    for (key, value, reason) in [
        ("conclusion", "failure", "it concluded failure"),
        ("conclusion", "skipped", "it concluded skipped"),
        ("name", "Other", ABSENT),
    ] {
        assert_refused(
            &allowed(&protected().green_job("Required", |job| job[key] = json!(value))),
            reason,
        );
    }
    for (key, value) in [
        ("head_sha", "0".repeat(40)),
        ("event", "pull_request".to_owned()),
    ] {
        let result = allowed(&protected().green_run(|run| run[key] = json!(value)));
        assert_refused(&result, ABSENT);
    }
    let external = json!({"context": "Required", "integration_id": 1234});
    let recorded = protected().rules(vec![
        json!({"type": "pull_request"}),
        required(&[external, dco()]),
    ]);
    assert_refused(
        &allowed(&recorded),
        "check Required from the GitHub App 1234",
    );
}

#[test]
fn release_gate_accepts_repeated_literal_pr_only_check_arguments() {
    let args = format!("velnor-ci release-gate --repo tailrocks/velnor-new --sha {GREEN}");
    let cli = Cli::try_parse_from(
        args.split_whitespace()
            .chain(["--pr-only-check=DCO", "--pr-only-check=-team's check"]),
    )
    .unwrap_or_else(|e| unreachable!("{e}"));
    let Command::ReleaseGate(args) = cli.command else {
        unreachable!()
    };
    assert_eq!(args.pr_only_check, ["DCO", "-team's check"]);
}
