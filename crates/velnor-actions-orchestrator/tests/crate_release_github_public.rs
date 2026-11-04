// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde_json::{Value, json};
use velnor_actions_orchestrator::{
    OrchestratorError,
    crate_release_github::{GitHubAdmissionRead, GitHubAdmissionRequest, verify_github_evidence},
};

const SOURCE: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const WORKFLOW: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

struct Fixture {
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
            json!({"full_name":"example/crate","default_branch":"main"})
        } else if route.ends_with("/actions/runs/12/approvals") {
            json!(self.approvals)
        } else if route.ends_with("/actions/runs/12") {
            json!({"id":12,"run_attempt":1,"head_sha":self.run_sha,"head_branch":"main","event":self.event,"path":".github/workflows/release.yml","repository":{"full_name":"example/crate"},"head_repository":{"full_name":"example/crate"}})
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
            json!({"total_count":1,"workflow_runs":[{"check_suite_id":123,"head_sha":source,"path":".github/workflows/ci.yml","status":"completed","conclusion":"success"}]})
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
            json!([{"number":7,"merged_at":"2026-11-03T15:38:50Z","merge_commit_sha":commit,"user":{"login":"author"},"base":{"ref":"main","repo":{"full_name":"example/crate"}},"head":{"ref":self.release_branch,"sha":WORKFLOW,"repo":{"full_name":"example/crate"}}}])
        } else if route.ends_with(SOURCE) && route.contains("/git/commits/") {
            json!({"sha":SOURCE,"tree":{"sha":SOURCE}})
        } else if route.ends_with(WORKFLOW) && route.contains("/git/commits/") {
            json!({"sha":WORKFLOW,"tree":{"sha":SOURCE}})
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
fn release_requires_valid_human_reviews_and_protected_environment() {
    let mut bad = Fixture::valid();
    bad.reviews[0]["user"]["type"] = json!("Bot");
    assert!(
        verify_github_evidence(&request(), &bad).is_err(),
        "bot cannot approve a release"
    );
    let mut bad = Fixture::valid();
    bad.reviews[0]["author_association"] = json!("NONE");
    assert!(
        verify_github_evidence(&request(), &bad).is_err(),
        "untrusted reviewer cannot approve a release"
    );
    let mut bad = Fixture::valid();
    bad.reviewer_permission = "read";
    assert!(
        verify_github_evidence(&request(), &bad).is_err(),
        "reviewer without write permission cannot authorize release"
    );
    let mut bad = Fixture::valid();
    bad.reviews[0]["state"] = json!("DISMISSED");
    assert!(
        verify_github_evidence(&request(), &bad).is_err(),
        "dismissed approval cannot authorize release"
    );
    let mut bad = Fixture::valid();
    let mut later = bad.reviews[0].clone();
    later["id"] = json!(0);
    later["state"] = json!("CHANGES_REQUESTED");
    later["submitted_at"] = json!("2026-11-03T15:01:00Z");
    bad.reviews.push(later);
    assert!(
        verify_github_evidence(&request(), &bad).is_err(),
        "a later review must prevail even with a smaller ID"
    );
    let mut bad = Fixture::valid();
    let mut bot = bad.reviews[0].clone();
    bot["id"] = json!(3);
    bot["user"] = json!({"login":"security-bot","id":56,"type":"Bot"});
    bot["state"] = json!("CHANGES_REQUESTED");
    bad.reviews.push(bot);
    assert!(
        verify_github_evidence(&request(), &bad).is_err(),
        "outstanding bot changes request cannot be ignored"
    );
    let mut bad = Fixture::valid();
    bad.reviews.clear();
    assert!(
        verify_github_evidence(&request(), &bad).is_err(),
        "unreviewed release must not publish"
    );
    let mut bad = Fixture::valid();
    bad.reviews[0]["submitted_at"] = json!("2026-11-03T15:38:50Z");
    assert!(
        verify_github_evidence(&request(), &bad).is_err(),
        "approval at merge time is not proven earlier"
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
            Ok(json!({"full_name":"example/crate","default_branch":"main"}))
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

#[test]
fn tag_lookup_distinguishes_absence_and_peeled_commit() {
    use velnor_actions_orchestrator::crate_release_github::read_github_tag_evidence;
    let mut fixture = TagFixture {
        reference: None,
        annotated: None,
        branch_readable: true,
    };
    assert_eq!(
        read_github_tag_evidence("example/crate", "crate-v1.0.0", &fixture).expect("read absence"),
        None
    );
    fixture.branch_readable = false;
    assert!(
        read_github_tag_evidence("example/crate", "crate-v1.0.0", &fixture).is_err(),
        "inaccessible refs cannot prove absence"
    );
    fixture.branch_readable = true;
    fixture.reference =
        Some(json!({"ref":"refs/tags/crate-v1.0.0","object":{"type":"commit","sha":SOURCE}}));
    assert_eq!(
        read_github_tag_evidence("example/crate", "crate-v1.0.0", &fixture)
            .expect("lightweight tag"),
        Some(SOURCE.into())
    );
    fixture.reference =
        Some(json!({"ref":"refs/tags/crate-v1.0.0","object":{"type":"tag","sha":WORKFLOW}}));
    fixture.annotated = Some(json!({"sha":WORKFLOW,"object":{"type":"commit","sha":SOURCE}}));
    assert_eq!(
        read_github_tag_evidence("example/crate", "crate-v1.0.0", &fixture).expect("annotated tag"),
        Some(SOURCE.into())
    );
    fixture.annotated = Some(json!({"sha":SOURCE,"object":{"type":"commit","sha":SOURCE}}));
    assert!(
        read_github_tag_evidence("example/crate", "crate-v1.0.0", &fixture).is_err(),
        "mismatched annotated object cannot identify a source"
    );
}

#[test]
fn ordinary_push_skips_without_weakening_release_admission() {
    use velnor_actions_orchestrator::crate_release_github::{
        GitHubAdmissionOutcome, admit_github_evidence,
    };
    let mut fixture = Fixture::valid();
    fixture.release_branch = "feature/example";
    fixture.reviews.clear();
    assert!(matches!(
        admit_github_evidence(&request(), &fixture),
        Ok(GitHubAdmissionOutcome::Skipped)
    ));
    fixture.release_branch = "release-plz-example";
    assert!(
        admit_github_evidence(&request(), &fixture).is_err(),
        "unreviewed release PR cannot skip admission"
    );
}

#[test]
fn environment_approval_binds_reviewer_policy_source_and_run() {
    use velnor_actions_orchestrator::crate_release_github::verify_environment_approval;
    let mut fixture = Fixture::valid();
    let mut input = request();
    assert!(verify_environment_approval(&input, &fixture).is_ok());
    fixture.approvals[0]["comment"] = json!("Ship it!");
    assert!(
        verify_environment_approval(&input, &fixture).is_err(),
        "generic approval cannot prove full policy review"
    );
    fixture.approvals = Fixture::valid().approvals;
    fixture.approvals[0]["user"]["id"] = json!(100);
    assert!(
        verify_environment_approval(&input, &fixture).is_err(),
        "wrong reviewer cannot authorize release"
    );
    fixture.approvals = Fixture::valid().approvals;
    fixture.approvals[0]["state"] = json!("rejected");
    assert!(verify_environment_approval(&input, &fixture).is_err());
    fixture.approvals = Fixture::valid().approvals;
    for wrong in [
        format!(
            "I reviewed the complete release policy at {WORKFLOW} and source {SOURCE}, and authorize this source for run 12."
        ),
        format!(
            "I reviewed the complete release policy at {SOURCE} and source {WORKFLOW}, and authorize this source for run 12."
        ),
        format!(
            "I reviewed the complete release policy at {SOURCE} and source {SOURCE}, and authorize this source for run 13."
        ),
    ] {
        fixture.approvals[0]["comment"] = json!(wrong);
        assert!(verify_environment_approval(&input, &fixture).is_err());
    }
    fixture.approvals = Fixture::valid().approvals;
    input.run_attempt = 2;
    assert!(
        verify_environment_approval(&input, &fixture).is_err(),
        "approval history lacks attempt identity"
    );
}

#[test]
fn candidate_becomes_authorized_only_after_matching_human_approval() {
    use velnor_actions_orchestrator::crate_release_github::authorize_github_candidate;
    let mut fixture = Fixture::valid();
    let input = request();
    let candidate = verify_github_evidence(&input, &fixture).expect("candidate");
    assert!(authorize_github_candidate(&input, &candidate, &fixture).is_ok());
    fixture.approvals.clear();
    assert!(
        authorize_github_candidate(&input, &candidate, &fixture).is_err(),
        "candidate alone cannot authorize release"
    );
    fixture.approvals = Fixture::valid().approvals;
    fixture.check["id"] = json!(2);
    assert!(
        authorize_github_candidate(&input, &candidate, &fixture).is_err(),
        "changed evidence cannot authorize stale candidate"
    );
}

#[test]
fn recovery_authorization_requires_owner_review_of_the_complete_policy() {
    use velnor_actions_orchestrator::crate_release_github::authorize_github_candidate;
    let mut fixture = Fixture::valid();
    fixture.event = "workflow_dispatch";
    fixture.run_sha = WORKFLOW;
    fixture.compare = "ahead";
    fixture.reviews.clear();
    let mut input = request();
    input.event = "workflow_dispatch".into();
    input.workflow_sha = WORKFLOW.into();
    input.recovery_sha = Some(SOURCE.into());
    let candidate = verify_github_evidence(&input, &fixture).expect("candidate");
    assert!(
        authorize_github_candidate(&input, &candidate, &fixture).is_err(),
        "approval for another workflow SHA cannot authorize recovery"
    );
    fixture.approvals[0]["comment"] = json!(format!(
        "I reviewed the complete release policy at {WORKFLOW} and authorize source {SOURCE} for run 12."
    ));
    assert!(
        authorize_github_candidate(&input, &candidate, &fixture).is_err(),
        "source authorization alone does not attest to source review"
    );
    fixture.approvals[0]["comment"] = json!(format!(
        "I reviewed the complete release policy at {WORKFLOW} and source {SOURCE}, and authorize this source for run 12."
    ));
    assert!(authorize_github_candidate(&input, &candidate, &fixture).is_ok());
}
