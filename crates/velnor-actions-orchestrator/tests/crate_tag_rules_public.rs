// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde_json::{Value, json};
use velnor_actions_orchestrator::{
    OrchestratorError,
    crate_release_github::{GitHubAdmissionRead, verify_immutable_tag_rules},
};

struct Fixture {
    detail: Value,
    fail: bool,
    full_page: bool,
}
impl GitHubAdmissionRead for Fixture {
    fn get(&self, route: &str) -> Result<Value, OrchestratorError> {
        if self.fail {
            return Err(OrchestratorError::Contract {
                problem: "unavailable".into(),
            });
        }
        if route.ends_with("/rulesets?targets=tag&includes_parents=true&per_page=100") {
            return Ok(if self.full_page {
                json!(vec![json!({"id":1}); 100])
            } else {
                json!([{"id":1}])
            });
        }
        if route.ends_with("/rulesets/1") {
            return Ok(self.detail.clone());
        }
        Err(OrchestratorError::Contract {
            problem: "unexpected route".into(),
        })
    }
    fn get_optional(&self, _: &str) -> Result<Option<Value>, OrchestratorError> {
        unreachable!()
    }
}
fn valid() -> Fixture {
    Fixture {
        fail: false,
        full_page: false,
        detail: json!({"id":1,"target":"tag","enforcement":"active","bypass_actors":[],"conditions":{"ref_name":{"include":["~ALL"],"exclude":[]}},"rules":[{"type":"update","parameters":{"update_allows_fetch_and_merge":false}},{"type":"deletion"}]}),
    }
}
#[test]
fn only_active_non_bypassable_immutable_tag_rules_are_accepted() {
    let mut fixture = valid();
    assert!(verify_immutable_tag_rules("example/demo", &fixture).is_ok());
    fixture.detail["rules"][0]
        .as_object_mut()
        .expect("rule object")
        .remove("parameters");
    assert!(verify_immutable_tag_rules("example/demo", &fixture).is_ok());
    fixture.detail["rules"] = json!([{"type":"non_fast_forward"},{"type":"deletion"}]);
    assert!(verify_immutable_tag_rules("example/demo", &fixture).is_err());
    fixture = valid();
    fixture.detail["bypass_actors"] = json!([{"actor_id":1}]);
    assert!(verify_immutable_tag_rules("example/demo", &fixture).is_err());
    fixture = valid();
    fixture.detail["conditions"]["ref_name"]["exclude"] = json!(["refs/tags/*"]);
    assert!(verify_immutable_tag_rules("example/demo", &fixture).is_err());
    fixture = valid();
    fixture.detail["enforcement"] = json!("disabled");
    assert!(verify_immutable_tag_rules("example/demo", &fixture).is_err());
    fixture = valid();
    fixture.detail["rules"] = json!(null);
    assert!(verify_immutable_tag_rules("example/demo", &fixture).is_err());
    fixture = valid();
    fixture.fail = true;
    assert!(verify_immutable_tag_rules("example/demo", &fixture).is_err());
    fixture = valid();
    fixture.detail["rules"][0]["parameters"]["update_allows_fetch_and_merge"] = json!(true);
    assert!(verify_immutable_tag_rules("example/demo", &fixture).is_err());
    fixture = valid();
    fixture.detail["id"] = json!(2);
    assert!(verify_immutable_tag_rules("example/demo", &fixture).is_err());
    fixture = valid();
    fixture.full_page = true;
    assert!(verify_immutable_tag_rules("example/demo", &fixture).is_err());
}
