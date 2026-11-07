// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::*;
use std::cell::Cell;
use velnor_actions_orchestrator::crate_release_github::{
    verify_oidc_qualification, verify_oidc_qualification_for_run,
};
use velnor_actions_orchestrator::crate_release_qualification_source::{
    ReviewedPullState, ReviewedSourceRequest,
};

struct QualificationFixture {
    base: Fixture,
    bad_rules: bool,
    changed_pull: bool,
    changed_environment: bool,
    pull_reads: Cell<u32>,
    environment_reads: Cell<u32>,
}

impl GitHubAdmissionRead for QualificationFixture {
    fn get(&self, route: &str) -> Result<Value, OrchestratorError> {
        if route.ends_with("/environments/release") {
            let count = self.environment_reads.get() + 1;
            self.environment_reads.set(count);
            let mut environment = self.base.get(route)?;
            if self.changed_environment && count > 3 {
                environment["can_admins_bypass"] = json!(true);
            }
            return Ok(environment);
        }
        if route.contains("/commits/") && route.contains("/pulls?") {
            return Ok(json!([{"number":7,"merged_at":"2026-11-03T15:38:50Z",
                "merge_commit_sha":SOURCE,"head":{"sha":WORKFLOW},
                "base":{"ref":"main","repo":{"id":101,"full_name":"example/crate"}}}]));
        }
        if route.ends_with("/pulls/7") {
            let count = self.pull_reads.get() + 1;
            self.pull_reads.set(count);
            return Ok(json!({"number":7,"state":"closed","merged":true,
                "merged_at":"2026-11-03T15:38:50Z","merge_commit_sha":SOURCE,
                "user":{"login":"author","id":if self.changed_pull && count > 1 { 9 } else { self.base.author_id }},
                "head":{"sha":WORKFLOW,"repo":{"id":101,"full_name":"example/crate"}},
                "base":{"ref":"main","repo":{"id":101,"full_name":"example/crate"}}}));
        }
        if route.contains("/rules/branches/") {
            return Ok(if self.bad_rules {
                json!([])
            } else {
                json!([
                    {"type":"required_status_checks","parameters":{"required_status_checks":[{"context":"Required"}]}},
                    {"type":"pull_request","parameters":{"required_review_thread_resolution":true}}
                ])
            });
        }
        self.base.get(route)
    }
    fn get_optional(&self, route: &str) -> Result<Option<Value>, OrchestratorError> {
        self.base.get_optional(route)
    }
}

fn fixture() -> (
    GitHubAdmissionRequest,
    ReviewedSourceRequest,
    QualificationFixture,
) {
    let mut request = request();
    request.event = "workflow_dispatch".into();
    request.recovery_sha = Some(SOURCE.into());
    let mut base = Fixture::valid();
    base.event = "workflow_dispatch";
    let reviewed = ReviewedSourceRequest {
        repository: "example/crate".into(),
        repository_id: 101,
        pull_request: 7,
        head_sha: WORKFLOW.into(),
        default_branch: "main".into(),
        required_workflow_path: ".github/workflows/ci.yml".into(),
        state: ReviewedPullState::Merged {
            merge_sha: SOURCE.into(),
            merge_tree_sha: SOURCE.into(),
        },
    };
    (
        request,
        reviewed,
        QualificationFixture {
            base,
            bad_rules: false,
            changed_pull: false,
            changed_environment: false,
            pull_reads: Cell::new(0),
            environment_reads: Cell::new(0),
        },
    )
}

#[test]
fn qualified_run_requires_reviewed_commit_checks_and_environment_approval() {
    let (request, reviewed, good) = fixture();
    assert!(verify_oidc_qualification(&request, &reviewed, &good).is_ok());
    for violation in [
        "branch-rules",
        "review",
        "check",
        "environment",
        "approval",
        "pull-changed",
        "environment-changed",
    ] {
        let (request, reviewed, mut fixture) = fixture();
        match violation {
            "branch-rules" => fixture.bad_rules = true,
            "review" => fixture.base.reviews.clear(),
            "check" => fixture.base.check["conclusion"] = json!("failure"),
            "environment" => fixture.base.allow_admin_bypass = true,
            "approval" => fixture.base.approvals.clear(),
            "pull-changed" => fixture.changed_pull = true,
            "environment-changed" => fixture.changed_environment = true,
            _ => unreachable!(),
        }
        assert!(
            verify_oidc_qualification(&request, &reviewed, &fixture).is_err(),
            "accepted {violation}"
        );
    }
}

#[test]
fn qualified_run_rejects_wrong_event_source_workflow_or_repository() {
    for violation in ["event", "source", "workflow", "repository", "unmerged"] {
        let (mut request, mut reviewed, fixture) = fixture();
        match violation {
            "event" => request.event = "push".into(),
            "source" => request.source_sha = WORKFLOW.into(),
            "workflow" => request.workflow_path = ".github/workflows/other.yml".into(),
            "repository" => reviewed.repository_id = 999,
            "unmerged" => reviewed.state = ReviewedPullState::OpenHead,
            _ => unreachable!(),
        }
        assert!(
            verify_oidc_qualification(&request, &reviewed, &fixture).is_err(),
            "accepted {violation}"
        );
    }
}

#[test]
fn qualification_resolves_the_reviewed_merge_from_the_run() {
    let (request, _, fixture) = fixture();
    assert!(verify_oidc_qualification_for_run(&request, &fixture).is_ok());
    let mut wrong = request;
    wrong.workflow_sha = WORKFLOW.into();
    assert!(verify_oidc_qualification_for_run(&wrong, &fixture).is_err());
}

struct AssociationFixture {
    base: QualificationFixture,
    duplicate: bool,
    changed: bool,
    reads: Cell<u32>,
}

impl GitHubAdmissionRead for AssociationFixture {
    fn get(&self, route: &str) -> Result<Value, OrchestratorError> {
        let mut value = self.base.get(route)?;
        if route.contains("/commits/") && route.contains("/pulls?") {
            let count = self.reads.get() + 1;
            self.reads.set(count);
            if self.duplicate {
                let duplicate = value[0].clone();
                value
                    .as_array_mut()
                    .ok_or_else(|| OrchestratorError::Contract {
                        problem: "fixture_association".into(),
                    })?
                    .push(duplicate);
            } else if self.changed && count > 1 {
                value[0]["head"]["sha"] = json!(SOURCE);
            }
        }
        Ok(value)
    }
    fn get_optional(&self, route: &str) -> Result<Option<Value>, OrchestratorError> {
        self.base.get_optional(route)
    }
}

#[test]
fn qualification_rejects_ambiguous_or_changed_run_merge_association() {
    for (duplicate, changed) in [(true, false), (false, true)] {
        let (request, _, base) = fixture();
        let api = AssociationFixture {
            base,
            duplicate,
            changed,
            reads: Cell::new(0),
        };
        assert!(verify_oidc_qualification_for_run(&request, &api).is_err());
    }
}
