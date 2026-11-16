// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use clap::Parser;
use serde_json::{Value, json};
use velnor_ci::cli::{Cli, Command};
use velnor_ci::github::{run_jobs_path, runs_path, statuses_path};

use super::{GREEN, Recorded, assert_refused, check, required};
use crate::support::repo;

const ABSENT: &str = "no job of a push run on main reports it";

fn dco() -> Value {
    json!({"context": "DCO", "integration_id": 343_649})
}

pub(super) const RULESET: &str = "/repos/tailrocks/velnor-new/rulesets/7";

pub(super) fn pr_rule() -> Value {
    json!({"type": "pull_request", "ruleset_id": 7, "ruleset_source_type": "Repository", "ruleset_source": "tailrocks/velnor-new"})
}

pub(super) fn ruleset() -> Value {
    json!({"id": 7, "source_type": "Repository", "source": "tailrocks/velnor-new", "target": "branch", "enforcement": "active", "bypass_actors": [], "rules": [{"type": "pull_request"}]})
}

pub(super) fn checks_path(page: u32) -> String {
    format!(
        "/repos/tailrocks/velnor-new/commits/{GREEN}/check-runs?filter=all&per_page=100&page={page}"
    )
}

pub(super) fn protected() -> Recorded {
    Recorded::new()
        .rules(vec![pr_rule(), required(&[check("Required"), dco()])])
        .set(RULESET.to_owned(), ruleset())
        .set(checks_path(1), json!({"total_count": 0, "check_runs": []}))
}

pub(super) fn allowed(recorded: &Recorded) -> Result<velnor_ci::gate::Verdict, String> {
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
fn pr_only_checks_with_push_jobs_are_refused_regardless_of_their_result() {
    for (status, conclusion) in [
        ("completed", json!("success")),
        ("completed", json!("failure")),
        ("completed", json!("skipped")),
        ("in_progress", Value::Null),
    ] {
        let recorded = protected().green_job("Required verdict", |job| {
            job["name"] = json!("DCO");
            job["status"] = json!(status);
            job["conclusion"] = conclusion.clone();
        });
        assert_refused(
            &allowed(&recorded),
            "PR-only check DCO has a push job or commit status",
        );
    }
}

#[test]
fn pr_only_checks_with_commit_statuses_are_refused_regardless_of_their_state() {
    for state in ["success", "failure", "pending"] {
        for context in ["DCO", "dco"] {
            let recorded = protected().set(
                statuses_path(&repo(), GREEN, 1),
                json!({"total_count": 1, "statuses": [{"context": context, "state": state}]}),
            );
            assert_refused(
                &allowed(&recorded),
                "PR-only check DCO has a push job or commit status",
            );
        }
    }
}

#[test]
fn pr_only_checks_with_push_jobs_on_other_branches_of_the_commit_are_refused() {
    for branch in [json!("other"), Value::Null] {
        let recorded = protected()
            .total(&runs_path(&repo(), GREEN, 1), 7)
            .set(
                runs_path(&repo(), GREEN, 2),
                json!({"total_count": 7, "workflow_runs": [{"id": 7, "event": "push", "status": "completed", "head_branch": branch, "head_sha": GREEN, "head_repository": {"id": 1_390_620_900}}]}),
            )
            .set(
                run_jobs_path(&repo(), 7, 1),
                json!({"total_count": 1, "jobs": [{"name": "DCO", "status": "completed", "conclusion": "success"}]}),
            );
        assert_refused(
            &allowed(&recorded),
            "PR-only check DCO has a push job or commit status",
        );
    }
}

#[test]
fn pr_only_exemptions_require_literal_case_and_whitespace_in_required_names() {
    for name in ["dco", " DCO", "DCO "] {
        let result = protected().gate_with(GREEN, &[name.to_owned()]);
        assert_refused(
            &result,
            &format!("PR-only check {name} is not required on branch main"),
        );
    }
}

#[test]
fn pr_only_exemptions_cannot_remove_every_required_push_check() {
    for (key, value) in [("conclusion", "failure"), ("name", "Other")] {
        let result = protected()
            .green_job("Required", |job| job[key] = json!(value))
            .gate_with(GREEN, &["DCO".to_owned(), "Required".to_owned()]);
        assert_refused(&result, "no required push check after PR-only exemptions");
    }
}

#[test]
fn pr_only_exemptions_require_live_required_names_and_a_pull_request_rule() {
    let stale = protected().rules(vec![pr_rule(), required(&[check("Required")])]);
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
    let recorded = protected().rules(vec![pr_rule(), required(&[external, dco()])]);
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
