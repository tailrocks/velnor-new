// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::*;
use velnor_actions_orchestrator::crate_release_github::verify_environment_approval;

struct Names {
    resource: &'static str,
    approval: &'static str,
    approval_id: u64,
}
impl GitHubAdmissionRead for Names {
    fn get(&self, route: &str) -> Result<Value, OrchestratorError> {
        let mut value = Fixture::valid().get(route)?;
        if route.ends_with("/environments/release") {
            value["name"] = json!(self.resource);
        } else if route.ends_with("/actions/runs/12/approvals") {
            value[0]["environments"][0]["name"] = json!(self.approval);
            value[0]["environments"][0]["id"] = json!(self.approval_id);
        }
        Ok(value)
    }
    fn get_optional(&self, route: &str) -> Result<Option<Value>, OrchestratorError> {
        self.get(route).map(Some)
    }
}
#[test]
fn environment_identity_accepts_case_variants_and_preserves_numeric_binding() {
    for (resource, approval) in [("ReLeAsE", "release"), ("release", "ReLeAsE")] {
        let api = Names {
            resource,
            approval,
            approval_id: 22,
        };
        let evidence = verify_github_evidence(&request(), &api);
        assert!(evidence.is_ok(), "{resource}: {evidence:?}");
        let approved = verify_environment_approval(&request(), &api);
        assert!(approved.is_ok(), "{approval}: {approved:?}");
    }
    let api = Names {
        resource: "releases",
        approval: "release",
        approval_id: 22,
    };
    assert!(verify_github_evidence(&request(), &api).is_err());
    assert!(verify_environment_approval(&request(), &api).is_err());
    for (approval, approval_id) in [("releases", 22), ("ReLeAsE", 23)] {
        let api = Names {
            resource: "release",
            approval,
            approval_id,
        };
        assert!(verify_environment_approval(&request(), &api).is_err());
    }
}
