// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde_json::{Value, json};
use std::cell::Cell;
use velnor_actions_orchestrator::{
    OrchestratorError,
    crate_release_github::{GitHubAdmissionRead, GitHubAdmissionRequest, verify_github_evidence},
};

const SOURCE: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const WORKFLOW: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

struct Fixture {
    author_id: u64,
    check: Value,
    extra_checks: Vec<Value>,
    reviews: Vec<Value>,
    env_rules: Vec<Value>,
    run_sha: &'static str,
    compare: &'static str,
    event: &'static str,
    release_branch: &'static str,
    allow_admin_bypass: bool,
    reviewer_id: u64,
    approvals: Vec<Value>,
    reviewer_permission: &'static str,
}

impl Fixture {
    fn valid() -> Self {
        Self {
            author_id: 54,
            check: json!({"id":1,"name":"Required","head_sha":SOURCE,"check_suite":{"id":123},"app":{"id":15368,"slug":"github-actions"},"status":"completed","conclusion":"success","started_at":"2026-11-03T15:00:00Z"}),
            extra_checks: Vec::new(),
            reviews: vec![
                json!({"id":1,"user":{"login":"reviewer","id":55,"type":"User"},"author_association":"MEMBER","state":"APPROVED","commit_id":WORKFLOW,"submitted_at":"2026-11-03T15:00:00Z"}),
            ],
            env_rules: vec![json!({"type":"branch","name":"main"})],
            run_sha: SOURCE,
            compare: "identical",
            event: "push",
            release_branch: "release-plz-test",
            allow_admin_bypass: false,
            reviewer_id: 99,
            reviewer_permission: "write",
            approvals: vec![
                json!({"state":"approved","user":{"type":"User","id":99},"comment":format!("I reviewed the complete release policy at {SOURCE} and source {SOURCE}, and authorize this source for run 12."),"environments":[{"id":22,"name":"release"}]}),
            ],
        }
    }
}

impl GitHubAdmissionRead for Fixture {
    fn get(&self, route: &str) -> Result<Value, OrchestratorError> {
        let v = if route == "repos/example/crate" {
            json!({"full_name":"example/crate","id":101,"owner":{"id":202,"login":"example"},"default_branch":"main"})
        } else if route.ends_with("/actions/runs/12/approvals") {
            json!(self.approvals)
        } else if route.ends_with("/actions/runs/12") {
            json!({"id":12,"run_attempt":1,"head_sha":self.run_sha,"head_branch":"main","event":self.event,"path":".github/workflows/release.yml","repository":{"full_name":"example/crate","id":101,"owner":{"id":202,"login":"example"}},"head_repository":{"full_name":"example/crate","id":101,"owner":{"id":202,"login":"example"}}})
        } else if route.contains("/compare/") {
            json!({"status":self.compare})
        } else if route.ends_with("/environments/release") {
            json!({"id":22,"name":"release","can_admins_bypass":self.allow_admin_bypass,"deployment_branch_policy":{"custom_branch_policies":true},"protection_rules":[{"type":"required_reviewers","reviewers":[{"type":"User","reviewer":{"id":self.reviewer_id}}]}]})
        } else if route.contains("deployment-branch-policies") {
            json!({"total_count":self.env_rules.len(),"branch_policies":self.env_rules})
        } else if route.contains("/actions/runs?") {
            let source = if route.contains(WORKFLOW) {
                WORKFLOW
            } else {
                SOURCE
            };
            json!({"total_count":1,"workflow_runs":[{"check_suite_id":123,"head_sha":source,"path":".github/workflows/ci.yml","status":"completed","conclusion":"success","event":"push","head_branch":"main","repository":{"id":101,"full_name":"example/crate","owner":{"id":202,"login":"example"}},"head_repository":{"id":101,"full_name":"example/crate","owner":{"id":202,"login":"example"}}}]})
        } else if route.contains("check-runs?") {
            let source = if route.contains(WORKFLOW) {
                WORKFLOW
            } else {
                SOURCE
            };
            let mut check = self.check.clone();
            check["head_sha"] = json!(source);
            let mut checks = vec![check];
            checks.extend(self.extra_checks.clone());
            json!({"total_count":checks.len(),"check_runs":checks})
        } else if route.contains("/commits/") && route.contains("/pulls?") {
            let commit = if route.contains(WORKFLOW) {
                WORKFLOW
            } else {
                SOURCE
            };
            json!([{"number":7,"merged_at":"2026-11-03T15:38:50Z","merge_commit_sha":commit,"user":{"login":"author","id":self.author_id},"base":{"ref":"main","repo":{"full_name":"example/crate","id":101,"owner":{"id":202,"login":"example"}}},"head":{"ref":self.release_branch,"sha":WORKFLOW,"repo":{"full_name":"example/crate","id":101,"owner":{"id":202,"login":"example"}}}}])
        } else if route.ends_with(SOURCE) && route.contains("/git/commits/") {
            json!({"sha":SOURCE,"tree":{"sha":SOURCE}})
        } else if route.ends_with(WORKFLOW) && route.contains("/git/commits/") {
            json!({"sha":WORKFLOW,"tree":{"sha":SOURCE}})
        } else if route.ends_with("/collaborators/security-bot/permission") {
            json!({"user":{"id":56,"type":"Bot"},"permission":"write"})
        } else if route.ends_with("/collaborators/read-only/permission") {
            json!({"user":{"id":57,"type":"User"},"permission":"read"})
        } else if route.ends_with("/collaborators/reviewer/permission") {
            json!({"user":{"id":55,"type":"User"},"permission":self.reviewer_permission})
        } else if route.contains("/pulls/7/reviews?") {
            json!(self.reviews)
        } else {
            return Err(OrchestratorError::Contract {
                problem: format!("unexpected route {route}"),
            });
        };
        Ok(v)
    }
    fn get_optional(&self, _route: &str) -> Result<Option<Value>, OrchestratorError> {
        Ok(None)
    }
}

fn request() -> GitHubAdmissionRequest {
    GitHubAdmissionRequest {
        repository: "example/crate".into(),
        default_branch: "main".into(),
        environment: "release".into(),
        trusted_reviewer_id: 99,
        workflow_path: ".github/workflows/release.yml".into(),
        required_workflow_path: ".github/workflows/ci.yml".into(),
        workflow_sha: SOURCE.into(),
        run_id: 12,
        run_attempt: 1,
        event: "push".into(),
        source_sha: SOURCE.into(),
        recovery_sha: None,
    }
}

#[test]
fn pending_required_check_is_distinct_from_failure_or_spoof() {
    let mut pending = Fixture::valid();
    pending.check["status"] = json!("queued");
    pending.check["conclusion"] = serde_json::Value::Null;
    pending.check["started_at"] = serde_json::Value::Null;
    assert!(
        matches!(verify_github_evidence(&request(), &pending), Err(OrchestratorError::Internal { problem }) if problem == "crate_release_required_pending")
    );
    pending.check["status"] = json!("completed");
    pending.check["conclusion"] = json!("failure");
    pending.check["started_at"] = json!("2026-11-03T15:00:00Z");
    assert!(
        matches!(verify_github_evidence(&request(), &pending), Err(OrchestratorError::Internal { problem }) if problem == "crate_release_required_failed")
    );
    pending.check["status"] = json!("queued");
    pending.check["started_at"] = serde_json::Value::Null;
    pending.check["app"]["id"] = json!(42);
    assert!(
        matches!(verify_github_evidence(&request(), &pending), Err(OrchestratorError::Internal { problem }) if problem == "crate_release_check_app")
    );
}

#[test]
fn release_requires_approved_source_checks_and_protected_environment() {
    let good = Fixture::valid();
    assert!(
        verify_github_evidence(&request(), &good).is_ok(),
        "reviewed source should produce a candidate"
    );
    let mut bad = Fixture::valid();
    bad.check["conclusion"] = json!("failure");
    assert!(
        verify_github_evidence(&request(), &bad).is_err(),
        "failed required check must not publish"
    );
    let mut bad = Fixture::valid();
    bad.check["id"] = json!(100);
    let mut latest = bad.check.clone();
    latest["id"] = json!(2);
    latest["started_at"] = json!("2026-11-03T15:01:00Z");
    latest["conclusion"] = json!("failure");
    bad.extra_checks.push(latest);
    assert!(
        verify_github_evidence(&request(), &bad).is_err(),
        "later failed Required must prevail over higher older ID"
    );
    let mut bad = Fixture::valid();
    let mut tied = bad.check.clone();
    tied["id"] = json!(2);
    bad.extra_checks.push(tied);
    assert!(
        verify_github_evidence(&request(), &bad).is_err(),
        "equal-second Required order is ambiguous"
    );
    let mut bad = Fixture::valid();
    bad.extra_checks.push(bad.check.clone());
    assert!(
        verify_github_evidence(&request(), &bad).is_err(),
        "duplicate check IDs must fail closed"
    );
    let mut bad = Fixture::valid();
    bad.check["app"]["id"] = json!(42);
    assert!(
        verify_github_evidence(&request(), &bad).is_err(),
        "spoofed required check must not publish"
    );
}

#[test]
fn release_requires_environment_protection() {
    let mut bad = Fixture::valid();
    bad.allow_admin_bypass = true;
    assert!(
        verify_github_evidence(&request(), &bad)
            .expect_err("admin bypass must fail")
            .to_string()
            .contains("environment_bypass")
    );
    let mut bad = Fixture::valid();
    bad.reviewer_id = 100;
    assert!(
        verify_github_evidence(&request(), &bad)
            .expect_err("untrusted reviewer must fail")
            .to_string()
            .contains("environment_reviewers")
    );
    let mut bad = Fixture::valid();
    bad.env_rules.push(json!({"type":"branch","name":"*"}));
    assert!(
        verify_github_evidence(&request(), &bad).is_err(),
        "unrestricted environment must not publish"
    );
    let mut bad = Fixture::valid();
    bad.run_sha = WORKFLOW;
    assert!(
        verify_github_evidence(&request(), &bad).is_err(),
        "different workflow source must not publish"
    );
    let mut bad = Fixture::valid();
    bad.compare = "diverged";
    assert!(
        verify_github_evidence(&request(), &bad).is_err(),
        "source outside main ancestry must not publish"
    );
}

#[test]
fn recovery_requires_the_exact_approved_source() {
    let mut fixture = Fixture::valid();
    fixture.event = "workflow_dispatch";
    fixture.run_sha = WORKFLOW;
    fixture.compare = "ahead";
    let mut input = request();
    input.event = "workflow_dispatch".into();
    input.workflow_sha = WORKFLOW.into();
    input.recovery_sha = Some(SOURCE.into());
    assert!(
        verify_github_evidence(&input, &fixture).is_ok(),
        "reviewed recovery should produce a candidate"
    );
    fixture.reviews.clear();
    assert!(
        verify_github_evidence(&input, &fixture).is_ok(),
        "recovery policy review occurs at environment approval"
    );
    fixture.reviews = Fixture::valid().reviews;
    fixture.run_sha = SOURCE;
    fixture.compare = "identical";
    input.workflow_sha = SOURCE.into();
    assert!(
        verify_github_evidence(&input, &fixture).is_ok(),
        "reviewed same-source recovery should produce a candidate"
    );
    fixture.reviews.clear();
    assert!(
        verify_github_evidence(&input, &fixture).is_ok(),
        "same-source recovery policy review occurs at environment approval"
    );
    fixture.reviews = Fixture::valid().reviews;
    fixture.run_sha = WORKFLOW;
    fixture.compare = "ahead";
    input.workflow_sha = WORKFLOW.into();
    input.recovery_sha = Some(WORKFLOW.into());
    assert!(
        verify_github_evidence(&input, &fixture).is_err(),
        "recovery must match the approved source"
    );
}

struct TagFixture {
    reference: Option<Value>,
    annotated: Option<Value>,
    branch_readable: bool,
}

impl GitHubAdmissionRead for TagFixture {
    fn get(&self, route: &str) -> Result<Value, OrchestratorError> {
        if route == "repos/example/crate" {
            Ok(
                json!({"full_name":"example/crate","id":101,"owner":{"id":202,"login":"example"},"default_branch":"main"}),
            )
        } else if route.ends_with("git/ref/heads/main") && self.branch_readable {
            Ok(json!({"ref":"refs/heads/main","object":{"type":"commit","sha":SOURCE}}))
        } else if route.contains("/git/tags/") {
            self.annotated
                .clone()
                .ok_or_else(|| OrchestratorError::Contract {
                    problem: "missing tag object".into(),
                })
        } else {
            Err(OrchestratorError::Contract {
                problem: "unreadable GitHub resource".into(),
            })
        }
    }
    fn get_optional(&self, _route: &str) -> Result<Option<Value>, OrchestratorError> {
        Ok(self.reference.clone())
    }
}

#[path = "crate_release_github_public/authorization.rs"]
mod authorization;

#[path = "crate_release_github_public/required_provenance.rs"]
mod required_provenance;

#[path = "crate_release_github_public/review_ties.rs"]
mod review_ties;

#[path = "crate_release_github_public/reviews.rs"]
mod reviews;

#[path = "crate_release_github_public/environment_names.rs"]
mod environment_names;
