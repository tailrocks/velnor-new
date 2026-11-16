// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use serde_json::{Value, json};
use velnor_ci::gate::{Verdict, release_gate};
use velnor_ci::github::{branch_path, compare_path, rules_path, run_jobs_path, runs_path};

use crate::support::{FakeGitHub, repo};

mod external_checks;
mod inputs;
mod pr_only;
mod pr_rules;
mod status;

pub(crate) const GATE: &str = include_str!("../data/gate.json");
pub(crate) const GREEN: &str = "21ec8b9ff9268c1e2445755d325906d26757babb";
pub(crate) const RUNNING: &str = "25fd566dd0950e7db7a3d1e58d760ad73ca92c90";
pub(crate) const RED: &str = "f4a062683f7e31e3c654c3beebaa6751463f6459";
pub(crate) const AHEAD: &str = "5839d07cc9d92794a514c686081c83f6c3cf95f7";
const GREEN_CI: u64 = 43_632_161_397;
const VERDICT: &str = "Required verdict";
const ONE_PASSED: &str = "check Required passed\n";
const BOTH_PASSED: &str = "check Required passed\ncheck Required verdict passed\n";

struct Recorded(BTreeMap<String, Value>);

impl Recorded {
    fn gate_with_budget(&self, limit: usize) -> Result<Verdict, String> {
        struct Budget<'a> {
            responses: &'a BTreeMap<String, Value>,
            left: std::cell::Cell<usize>,
        }
        impl velnor_ci::github::GitHub for Budget<'_> {
            fn get(&self, path: &str) -> Result<String, velnor_ci::error::CiError> {
                let left =
                    self.left.get().checked_sub(1).ok_or_else(|| {
                        velnor_ci::error::CiError::new("request budget exhausted")
                    })?;
                self.left.set(left);
                self.responses
                    .get(path)
                    .map(Value::to_string)
                    .ok_or_else(|| {
                        velnor_ci::error::CiError::new(format!("no recorded response for {path}"))
                    })
            }
        }
        release_gate(
            &Budget {
                responses: &self.0,
                left: std::cell::Cell::new(limit),
            },
            &repo(),
            GREEN,
            &["DCO".to_owned()],
        )
        .map_err(|e| e.to_string())
    }
    fn new() -> Self {
        Self(serde_json::from_str(GATE).unwrap_or_else(|e| unreachable!("{e}")))
    }

    fn set(mut self, path: String, body: Value) -> Self {
        self.0.insert(path, body);
        self
    }

    fn edit(mut self, path: &str, change: impl FnOnce(&mut Value)) -> Self {
        change(self.0.get_mut(path).unwrap_or_else(|| unreachable!()));
        self
    }

    fn rules(self, rules: Vec<Value>) -> Self {
        self.set(rules_path(&repo(), "main", 1), Value::Array(rules))
    }

    fn requiring(self, checks: &[Value]) -> Self {
        self.rules(vec![json!({"type": "deletion"}), required(checks)])
    }

    fn green_run(self, change: impl Fn(&mut Value)) -> Self {
        self.edit(&runs_path(&repo(), GREEN, 1), |runs| {
            for run in each(&mut runs["workflow_runs"]) {
                if run["id"] == json!(GREEN_CI) {
                    change(run);
                }
            }
        })
    }

    fn green_job(self, name: &str, change: impl Fn(&mut Value)) -> Self {
        self.edit(&run_jobs_path(&repo(), GREEN_CI, 1), |jobs| {
            for job in each(&mut jobs["jobs"]) {
                if job["name"] == json!(name) {
                    change(job);
                }
            }
        })
    }

    fn protection(self, branch: &str, level: &str, contexts: &[&str], checks: &Value) -> Self {
        self.set(
            branch_path(&repo(), branch),
            json!({"protection": {"enabled": level != "off", "required_status_checks": {"enforcement_level": level, "contexts": contexts, "checks": checks}}}),
        )
    }

    fn total(self, path: &str, total: usize) -> Self {
        self.edit(path, |page| page["total_count"] = json!(total))
    }

    fn gate(&self, sha: &str) -> Result<Verdict, String> {
        self.gate_with(sha, &[])
    }

    fn gate_with(&self, sha: &str, pr_only: &[String]) -> Result<Verdict, String> {
        let mut fake = FakeGitHub::default();
        for (path, body) in &self.0 {
            fake.respond(path.clone(), &body.to_string());
        }
        release_gate(&fake, &repo(), sha, pr_only).map_err(|e| e.to_string())
    }
}

fn each(list: &mut Value) -> impl Iterator<Item = &mut Value> {
    list.as_array_mut()
        .unwrap_or_else(|| unreachable!())
        .iter_mut()
}

fn check(context: &str) -> Value {
    json!({"context": context})
}

fn required(checks: &[Value]) -> Value {
    json!({"type": "required_status_checks", "parameters": {"strict_required_status_checks_policy": true, "do_not_enforce_on_create": false, "required_status_checks": checks}, "ruleset_source_type": "Repository", "ruleset_source": "tailrocks/velnor-new", "ruleset_id": 24_396_608})
}

fn assert_refused(said: &Result<Verdict, String>, part: &str) {
    assert!(said.as_ref().is_err_and(|e| e.contains(part)), "{said:?}");
}

fn both() -> Recorded {
    Recorded::new().requiring(&[check("Required"), check(VERDICT)])
}

fn verdict(ready: bool, sha: &str, checks: &str) -> Verdict {
    Verdict {
        ready,
        report: format!("commit {sha} is on main\n{checks}ready={ready}\n"),
    }
}

fn concluded(sha: &str, check: &str, conclusion: &str) -> Result<Verdict, String> {
    Err(format!(
        "check {check} did not pass on commit {sha}: it concluded {conclusion}"
    ))
}

fn absent(check: &str) -> String {
    format!("check {check} did not pass on commit {GREEN}: no job of a push run on main reports it")
}

fn assert_not_counted(case: &str, change: impl Fn(&mut Value)) {
    let said = both().green_run(change).gate(GREEN);
    let both_absent = format!("{}\n{}", absent("Required"), absent(VERDICT));
    assert_eq!(said, Err(both_absent), "{case}");
}

fn failing(job: &mut Value) {
    job["conclusion"] = json!("failure");
}

#[test]
fn a_commit_on_main_whose_two_required_checks_passed_in_its_push_run_is_ready() {
    assert_eq!(both().gate(GREEN), Ok(verdict(true, GREEN, BOTH_PASSED)));
    assert_eq!(
        Recorded::new().gate(GREEN),
        Ok(verdict(true, GREEN, ONE_PASSED))
    );
}

#[test]
fn every_required_check_must_pass_whether_one_ruleset_or_several_require_it() {
    let one_rule = both().green_job(VERDICT, failing);
    let two_rules = Recorded::new()
        .rules(vec![
            required(&[check("Required")]),
            required(&[check(VERDICT)]),
        ])
        .green_job(VERDICT, failing);
    for recorded in [one_rule, two_rules] {
        assert_eq!(recorded.gate(GREEN), concluded(GREEN, VERDICT, "failure"));
    }
    assert_eq!(
        both().gate(RED),
        Err(format!(
            "check Required did not pass on commit {RED}: it concluded failure\ncheck Required verdict did not pass on commit {RED}: it concluded failure"
        ))
    );
}

#[test]
fn a_required_check_still_running_gives_ready_false_and_a_failed_one_beside_it_exits_1() {
    let not_started = "check Required has not started\ncheck Required verdict has not started\n";
    assert_eq!(
        both().gate(RUNNING),
        Ok(verdict(false, RUNNING, not_started))
    );
    let running = both()
        .green_run(|run| run["status"] = json!("in_progress"))
        .green_job("Required", |job| {
            job["status"] = json!("in_progress");
            job["conclusion"] = Value::Null;
        });
    let in_progress = "check Required is in_progress\ncheck Required verdict passed\n";
    assert_eq!(running.gate(GREEN), Ok(verdict(false, GREEN, in_progress)));
    let failed_beside = running.green_job(VERDICT, failing);
    assert_eq!(
        failed_beside.gate(GREEN),
        concluded(GREEN, VERDICT, "failure")
    );
}

#[test]
fn a_required_check_that_is_missing_or_skipped_or_did_not_succeed_is_refused() {
    for conclusion in ["skipped", "cancelled", "neutral", "timed_out", "failure"] {
        let said = both()
            .green_job("Required", |job| job["conclusion"] = json!(conclusion))
            .gate(GREEN);
        assert_eq!(said, concluded(GREEN, "Required", conclusion));
    }
    let said = both()
        .green_job("Required", |job| job["conclusion"] = Value::Null)
        .gate(GREEN);
    assert_eq!(said, concluded(GREEN, "Required", "without a result"));
    let said = Recorded::new()
        .requiring(&[check("Required"), check("DCO")])
        .gate(GREEN);
    assert_eq!(said, Err(absent("DCO")));
}

#[test]
fn a_required_check_from_a_run_that_is_not_a_push_is_refused() {
    for event in [
        "pull_request",
        "pull_request_target",
        "workflow_dispatch",
        "workflow_run",
        "schedule",
        "merge_group",
    ] {
        assert_not_counted(event, |run| run["event"] = json!(event));
    }
}

#[test]
fn a_required_check_from_a_push_run_on_another_branch_or_commit_is_refused() {
    for branch in [json!("fast/724-forge"), json!("main2"), Value::Null] {
        assert_not_counted(&branch.to_string(), |run| {
            run["head_branch"] = branch.clone();
        });
    }
    assert_not_counted("another commit", |run| run["head_sha"] = json!(RED));
}

#[test]
fn a_required_check_from_a_fork_whose_branch_is_named_main_is_refused() {
    let fork = json!({"id": 1_390_999_999, "full_name": "someone/velnor-new"});
    for (event, head) in [
        ("push", fork.clone()),
        ("pull_request", fork),
        ("push", Value::Null),
    ] {
        assert_not_counted(&format!("{event} {head}"), |run| {
            run["event"] = json!(event);
            run["head_repository"] = head.clone();
        });
    }
}

#[test]
fn a_required_check_the_rules_or_the_protection_tie_to_another_github_app_is_refused() {
    let actions = json!({"context": "Required", "integration_id": 15368});
    let any_app = json!([{"context": VERDICT, "app_id": -1}]);
    let any_app = Recorded::new().protection("main", "everyone", &[VERDICT], &any_app);
    for (recorded, passed) in [
        (Recorded::new().requiring(&[actions]), ONE_PASSED),
        (any_app, BOTH_PASSED),
    ] {
        assert_eq!(recorded.gate(GREEN), Ok(verdict(true, GREEN, passed)));
    }
    let other = json!({"context": "Required", "integration_id": 9426});
    let dco = json!([{"context": "DCO", "app_id": 1861}]);
    for (recorded, check, app) in [
        (Recorded::new().requiring(&[other]), "Required", 9426),
        (
            Recorded::new().protection("main", "everyone", &["DCO"], &dco),
            "DCO",
            1861,
        ),
    ] {
        assert_eq!(
            recorded.gate(GREEN),
            Err(format!(
                "branch main requires check {check} from the GitHub App {app}, and only a check from GitHub Actions (15368) can be tied to a push run"
            ))
        );
    }
}

#[test]
fn a_commit_ahead_of_or_diverged_from_the_default_branch_is_refused_even_with_green_checks() {
    let off_main = |sha: &str, status: &str| {
        format!("commit {sha} is not on the default branch main (comparison: {status})")
    };
    for (status, ahead_by, behind_by) in [("ahead", 1, 0), ("diverged", 4, 2)] {
        let comparison = json!({"status": status, "ahead_by": ahead_by, "behind_by": behind_by});
        let path = compare_path(&repo(), "main", GREEN);
        let said = both().set(path, comparison).gate(GREEN);
        assert_eq!(said, Err(off_main(GREEN, status)));
    }
    assert_refused(
        &both().gate(AHEAD),
        &format!("{}\n", off_main(AHEAD, "ahead")),
    );
}

#[test]
fn a_check_that_the_rules_or_classic_branch_protection_require_counts_and_none_refuses() {
    let none = Recorded::new().requiring(&[]).gate(GREEN);
    assert_refused(
        &none,
        "requires no status check in its rules or its branch protection",
    );
    let verdict_check = json!([{"context": VERDICT, "app_id": 15368}]);
    let enforced = Recorded::new()
        .protection("main", "everyone", &[VERDICT], &verdict_check)
        .green_job(VERDICT, failing);
    assert_eq!(enforced.gate(GREEN), concluded(GREEN, VERDICT, "failure"));
    let off = Recorded::new()
        .protection("main", "off", &[VERDICT], &verdict_check)
        .green_job(VERDICT, failing);
    let only_required = Ok(verdict(true, GREEN, ONE_PASSED));
    assert_eq!(off.gate(GREEN), only_required);
    let classic_only = Recorded::new()
        .rules(vec![json!({"type": "deletion"})])
        .protection("main", "non_admins", &["Required"], &json!([]));
    assert_eq!(classic_only.gate(GREEN), only_required);
    let unknown = Recorded::new().set(
        branch_path(&repo(), "main"),
        json!({"name": "main", "protected": true}),
    );
    assert_refused(&unknown.gate(GREEN), "cannot read GitHub branch response");
}

#[test]
fn a_failing_required_check_on_a_later_page_of_rules_runs_or_jobs_is_seen() {
    let mut full_page = vec![json!({"type": "deletion"}); 99];
    full_page.push(required(&[check("Required")]));
    let rules_page_2 = Recorded::new()
        .rules(full_page)
        .set(
            rules_path(&repo(), "main", 2),
            json!([required(&[check(VERDICT)])]),
        )
        .green_job(VERDICT, failing);
    let extra_run = json!({"id": 7, "event": "push", "status": "completed", "head_branch": "main", "head_sha": GREEN, "head_repository": {"id": 1_390_620_900}});
    let failed_job =
        |name: &str| json!({"name": name, "status": "completed", "conclusion": "failure"});
    let runs_page_2 = both()
        .total(&runs_path(&repo(), GREEN, 1), 7)
        .set(
            runs_path(&repo(), GREEN, 2),
            json!({"total_count": 7, "workflow_runs": [extra_run]}),
        )
        .set(
            run_jobs_path(&repo(), 7, 1),
            json!({"total_count": 1, "jobs": [failed_job("Required")]}),
        );
    let jobs_page_2 = both().total(&run_jobs_path(&repo(), GREEN_CI, 1), 4).set(
        run_jobs_path(&repo(), GREEN_CI, 2),
        json!({"total_count": 4, "jobs": [failed_job(VERDICT)]}),
    );
    for recorded in [rules_page_2, runs_page_2, jobs_page_2] {
        assert_refused(&recorded.gate(GREEN), ": it concluded failure");
    }
    let short = both().total(&runs_path(&repo(), GREEN, 1), 7).set(
        runs_path(&repo(), GREEN, 2),
        json!({"total_count": 7, "workflow_runs": []}),
    );
    let said = short.gate(GREEN);
    assert_refused(&said, "GitHub lists 7 workflow runs and returned 6");
}
