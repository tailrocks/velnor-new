// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::*;

#[test]
fn authorization_binds_repository_and_owner_ids_to_the_run() {
    use velnor_actions_orchestrator::crate_release_github::authorize_github_candidate;
    struct IdFixture {
        change: Option<(&'static str, &'static str, Value)>,
    }
    impl GitHubAdmissionRead for IdFixture {
        fn get(&self, route: &str) -> Result<Value, OrchestratorError> {
            let mut value = Fixture::valid().get(route)?;
            let key = if route == "repos/example/crate" {
                value["id"] = json!(101);
                value["owner"] = json!({"id":202,"login":"example"});
                "repo"
            } else if route.ends_with("/actions/runs/12") {
                for field in ["repository", "head_repository"] {
                    value[field]["id"] = json!(101);
                    value[field]["owner"] = json!({"id":202,"login":"example"});
                }
                "run"
            } else {
                "other"
            };
            if let Some((target, pointer, replacement)) = &self.change
                && *target == key
            {
                *value.pointer_mut(pointer).expect("field") = replacement.clone();
            }
            Ok(value)
        }
        fn get_optional(&self, _: &str) -> Result<Option<Value>, OrchestratorError> {
            Ok(None)
        }
    }
    let input = request();
    let valid = IdFixture { change: None };
    let candidate = verify_github_evidence(&input, &valid).expect("candidate");
    let authority = authorize_github_candidate(&input, &candidate, &valid).expect("authorization");
    assert_eq!(
        (authority.repository_id(), authority.owner_id()),
        (101, 202)
    );
    for (target, pointer) in [
        ("repo", "/id"),
        ("repo", "/owner/id"),
        ("run", "/repository/id"),
        ("run", "/repository/owner/id"),
        ("run", "/head_repository/id"),
        ("run", "/head_repository/owner/id"),
    ] {
        for replacement in [json!(0), json!(999), Value::Null, json!("101")] {
            let changed = IdFixture {
                change: Some((target, pointer, replacement)),
            };
            assert!(
                authorize_github_candidate(&input, &candidate, &changed).is_err(),
                "{target}{pointer}"
            );
        }
    }
}

#[test]
fn authorization_rejects_repository_transfer_during_approval() {
    use velnor_actions_orchestrator::crate_release_github::authorize_github_candidate;
    struct Transfer {
        approved: Cell<bool>,
    }
    impl GitHubAdmissionRead for Transfer {
        fn get(&self, route: &str) -> Result<Value, OrchestratorError> {
            let mut value = Fixture::valid().get(route)?;
            if route.ends_with("/actions/runs/12/approvals") {
                self.approved.set(true);
            }
            if self.approved.get() {
                if route == "repos/example/crate" {
                    value["id"] = json!(999);
                }
                if route.ends_with("/actions/runs/12") {
                    value["repository"]["id"] = json!(999);
                    value["head_repository"]["id"] = json!(999);
                }
            }
            Ok(value)
        }
        fn get_optional(&self, _: &str) -> Result<Option<Value>, OrchestratorError> {
            Ok(None)
        }
    }
    let input = request();
    let fixture = Transfer {
        approved: Cell::new(false),
    };
    let candidate = verify_github_evidence(&input, &fixture).expect("candidate");
    assert!(authorize_github_candidate(&input, &candidate, &fixture).is_err());
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
fn environment_approval_uses_the_validated_environment_identity() {
    use velnor_actions_orchestrator::crate_release_github::verify_environment_approval;
    struct ChangingEnvironment {
        base: Fixture,
        reads: Cell<usize>,
        first_id: Value,
    }
    impl GitHubAdmissionRead for ChangingEnvironment {
        fn get(&self, route: &str) -> Result<Value, OrchestratorError> {
            if route.ends_with("/environments/release") {
                let reads = self.reads.get();
                self.reads.set(reads + 1);
                if reads > 0 {
                    return Ok(json!({"id":44,"name":"release","can_admins_bypass":true}));
                }
                let mut env = self.base.get(route)?;
                if self.first_id.is_null() {
                    env.as_object_mut().expect("environment").remove("id");
                } else {
                    env["id"] = self.first_id.clone();
                }
                return Ok(env);
            }
            self.base.get(route)
        }
        fn get_optional(&self, route: &str) -> Result<Option<Value>, OrchestratorError> {
            self.get(route).map(Some)
        }
    }
    let mut fixture = Fixture::valid();
    fixture.approvals[0]["environments"][0]["id"] = json!(44);
    let api = ChangingEnvironment {
        base: fixture,
        reads: Cell::new(0),
        first_id: json!(22),
    };
    assert!(
        verify_environment_approval(&request(), &api).is_err(),
        "an approval for an unverified environment ID must be rejected"
    );
    for id in [Value::Null, json!(0), json!("22")] {
        let api = ChangingEnvironment {
            base: Fixture::valid(),
            reads: Cell::new(0),
            first_id: id,
        };
        assert!(verify_environment_approval(&request(), &api).is_err());
    }
    let api = ChangingEnvironment {
        base: Fixture::valid(),
        reads: Cell::new(0),
        first_id: json!(22),
    };
    assert!(verify_environment_approval(&request(), &api).is_ok());
    assert_eq!(api.reads.get(), 1, "approval uses the validated response");
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

#[test]
fn environment_approval_allows_unrelated_protection_history_without_losing_human_authority() {
    use velnor_actions_orchestrator::crate_release_github::verify_environment_approval;
    let mut fixture = Fixture::valid();
    let trusted = fixture.approvals[0].clone();
    let extra = json!({"state":"approved","user":{"type":"Bot","id":501},"comment":"rule passed","environments":[{"id":22,"name":"release"}]});
    for history in [
        vec![trusted.clone(), extra.clone()],
        vec![extra.clone(), trusted.clone()],
    ] {
        fixture.approvals = history;
        assert!(verify_environment_approval(&request(), &fixture).is_ok());
    }
    let mut multi = trusted.clone();
    multi["environments"]
        .as_array_mut()
        .expect("environments")
        .push(json!({"id":23,"name":"other"}));
    fixture.approvals = vec![extra.clone(), multi];
    assert!(verify_environment_approval(&request(), &fixture).is_ok());
    for history in [vec![extra.clone()], vec![trusted.clone(), trusted.clone()]] {
        fixture.approvals = history;
        assert!(verify_environment_approval(&request(), &fixture).is_err());
    }
    for (field, value) in [
        ("state", json!("rejected")),
        ("user", json!({"type":"Bot","id":99})),
        ("comment", json!("wrong")),
        (
            "environments",
            json!([{"id":22,"name":"release"},{"id":22,"name":"release"}]),
        ),
    ] {
        let mut invalid = trusted.clone();
        invalid[field] = value;
        fixture.approvals = vec![extra.clone(), invalid];
        assert!(
            verify_environment_approval(&request(), &fixture).is_err(),
            "{field}"
        );
    }
    for (field, value) in [
        ("state", json!("rejected")),
        ("user", Value::Null),
        ("environments", json!([{"id":22,"name":"wrong"}])),
        ("environments", json!([{"id":999,"name":"release"}])),
    ] {
        let mut invalid = extra.clone();
        invalid[field] = value;
        fixture.approvals = vec![trusted.clone(), invalid];
        assert!(verify_environment_approval(&request(), &fixture).is_err());
    }
    let mut conflicting = trusted.clone();
    conflicting["state"] = json!("rejected");
    fixture.approvals = vec![trusted, conflicting, extra];
    assert!(verify_environment_approval(&request(), &fixture).is_err());
}
