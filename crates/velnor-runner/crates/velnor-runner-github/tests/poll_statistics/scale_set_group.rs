// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde_json::{Value, json};
use velnor_runner_github::{
    ScaleSetByName, ScaleSetCreate, ScaleSetFound, SessionError, WireError,
    create_runner_scale_set, get_runner_scale_set, product_create_labels,
};

use super::common::Script;

const NAME: &str = "ubuntu-26.04-scale-set";
const GROUP: i64 = 1;

fn scale_set(group: Option<Value>) -> Value {
    let mut body = json!({
        "id": 10,
        "name": NAME,
        "labels": [
            {"name": "velnor", "type": "System"},
            {"name": NAME, "type": "System"}
        ],
        "runnerSetting": {"disableUpdate": true}
    });
    if let Some(group) = group {
        body["runnerGroupId"] = group;
    }
    body
}

fn create(body: &Value) -> Result<i64, SessionError> {
    let labels = product_create_labels();
    let mut script = Script::once(200, &body.to_string());
    create_runner_scale_set(
        &mut script,
        &ScaleSetCreate {
            name: NAME,
            runner_group_id: GROUP,
            labels: &labels,
            admin_token: "token",
        },
    )
    .map(|view| view.id)
}

fn find(body: &Value) -> Result<ScaleSetFound, SessionError> {
    let expected = vec!["velnor".to_owned(), NAME.to_owned()];
    let page = json!({"count": 1, "value": [body]});
    let mut script = Script::once(200, &page.to_string());
    get_runner_scale_set(
        &mut script,
        &ScaleSetByName {
            runner_group_id: GROUP,
            name: NAME,
            expected_labels: &expected,
            admin_token: "token",
        },
    )
}

fn assert_rejected<T: std::fmt::Debug>(result: Result<T, SessionError>) {
    assert!(
        matches!(
            result,
            Err(SessionError::Wire(WireError::RegistrationRejected))
        ),
        "a foreign runner group must be rejected, got {result:?}"
    );
}

#[test]
fn create_response_naming_another_runner_group_is_rejected() {
    assert_rejected(create(&scale_set(Some(json!(99)))));
}

#[test]
fn existing_lookup_naming_another_runner_group_is_rejected() {
    assert_rejected(find(&scale_set(Some(json!(99)))));
}

#[test]
fn matching_or_omitted_runner_group_is_accepted() {
    for group in [Some(json!(GROUP)), None, Some(Value::Null)] {
        assert_eq!(create(&scale_set(group.clone())).expect("create"), 10);
        assert!(matches!(
            find(&scale_set(group)).expect("find"),
            ScaleSetFound::Found(_)
        ));
    }
}
