// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::verify_rehearsal_approval;
use crate::{
    OrchestratorError, crate_release_github::GitHubAdmissionRead,
    crate_release_qualification_witness::WitnessRequest, internal::internal,
};
use serde_json::{Value, json};
use std::{
    cell::{Cell, RefCell},
    collections::BTreeMap,
};

struct Fixture {
    request: WitnessRequest,
    values: RefCell<BTreeMap<String, Value>>,
    reads: Cell<usize>,
    mutate: Cell<bool>,
}
impl Fixture {
    fn new() -> Self {
        let request = WitnessRequest {
            repository: "acme/example".into(),
            repository_id: 12,
            workflow_sha: "a".repeat(40),
            workflow_path: ".github/workflows/release-rehearsal.yml".into(),
            run_id: 33,
            run_attempt: 1,
            job_id: 44,
            environment: "staging".into(),
            reviewer_id: 55,
            script: "verify/run.sh".into(),
            source_sha256: "b".repeat(64),
            candidate_pr: 66,
            candidate_sha: "c".repeat(40),
        };
        let repo = "repos/acme/example";
        let values = BTreeMap::from([
            (
                repo.into(),
                json!({"id":12,"full_name":"acme/example","default_branch":"main"}),
            ),
            (
                format!("{repo}/branches/main"),
                json!({"name":"main","protected":true,"commit":{"sha":request.workflow_sha}}),
            ),
            (
                format!("{repo}/actions/runs/33/attempts/1"),
                json!({"id":33,"run_attempt":1,"event":"workflow_dispatch","head_sha":request.workflow_sha,"head_branch":"main","path":request.workflow_path,"repository":{"id":12},"head_repository":{"id":12},"status":"completed","conclusion":"success"}),
            ),
            (
                format!("{repo}/environments/staging"),
                json!({"id":77,"name":"staging","can_admins_bypass":false,"deployment_branch_policy":{"protected_branches":true,"custom_branch_policies":false},"protection_rules":[{"type":"required_reviewers","reviewers":[{"type":"User","reviewer":{"type":"User","id":55}}]}]}),
            ),
            (
                format!("{repo}/actions/runs/33/approvals"),
                json!([{"state":"approved","user":{"type":"User","id":55},"environments":[{"id":77,"name":"staging"}]}]),
            ),
        ]);
        Self {
            request,
            values: RefCell::new(values),
            reads: Cell::new(0),
            mutate: Cell::new(false),
        }
    }
}
impl GitHubAdmissionRead for Fixture {
    fn get(&self, route: &str) -> Result<Value, OrchestratorError> {
        let count = self.reads.get();
        self.reads.set(count + 1);
        let mut value = self
            .values
            .borrow()
            .get(route)
            .cloned()
            .ok_or_else(|| internal("unexpected_fixture_route"))?;
        if self.mutate.get() && count >= 5 && route.ends_with("/environments/staging") {
            value["can_admins_bypass"] = true.into();
        }
        Ok(value)
    }
    fn get_optional(&self, route: &str) -> Result<Option<Value>, OrchestratorError> {
        self.get(route).map(Some)
    }
}
#[test]
fn approval_requires_real_matching_human_and_nonbypassable_environment() {
    let f = Fixture::new();
    assert!(verify_rehearsal_approval(&f.request, "main", &f).is_ok());
    f.values
        .borrow_mut()
        .get_mut("repos/acme/example/environments/staging")
        .expect("fixture")["protection_rules"][0]["reviewers"]
        .as_array_mut()
        .expect("reviewers")
        .push(json!({"type":"User","reviewer":{"type":"User","id":56}}));
    assert!(verify_rehearsal_approval(&f.request, "main", &f).is_ok());
    f.values
        .borrow_mut()
        .get_mut("repos/acme/example/environments/staging")
        .expect("fixture")["can_admins_bypass"] = true.into();
    assert!(verify_rehearsal_approval(&f.request, "main", &f).is_err());
    f.values
        .borrow_mut()
        .get_mut("repos/acme/example/environments/staging")
        .expect("fixture")["can_admins_bypass"] = false.into();
    let route = "repos/acme/example/actions/runs/33/approvals";
    let original = f.values.borrow().get(route).expect("fixture").clone();
    f.values.borrow_mut().insert(route.into(), json!([]));
    assert!(verify_rehearsal_approval(&f.request, "main", &f).is_err());
    f.values.borrow_mut().insert(
        route.into(),
        json!([original[0].clone(), original[0].clone()]),
    );
    assert!(verify_rehearsal_approval(&f.request, "main", &f).is_err());
    f.values.borrow_mut().insert(route.into(), original);
    f.values.borrow_mut().get_mut(route).expect("fixture")[0]["user"]["id"] = 56.into();
    assert!(verify_rehearsal_approval(&f.request, "main", &f).is_err());
    f.values.borrow_mut().get_mut(route).expect("fixture")[0]["user"]["id"] = 55.into();
    f.values
        .borrow_mut()
        .get_mut("repos/acme/example/actions/runs/33/approvals")
        .expect("fixture")[0]["environments"][0]["id"] = 78.into();
    assert!(verify_rehearsal_approval(&f.request, "main", &f).is_err());
}
#[test]
fn approval_rejects_reruns_and_policy_change_during_verification() {
    let mut f = Fixture::new();
    f.request.run_attempt = 2;
    assert!(verify_rehearsal_approval(&f.request, "main", &f).is_err());
    f.request.run_attempt = 1;
    f.mutate.set(true);
    f.reads.set(0);
    assert!(verify_rehearsal_approval(&f.request, "main", &f).is_err());
}
