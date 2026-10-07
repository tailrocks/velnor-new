//! Scale set get and create. No sockets.

#[path = "regwire/mod.rs"]
mod regwire;

use velnor_runner_github::{
    CAPACITY_HEADER, CreateLabel, Method, ScaleSetById, ScaleSetByName, ScaleSetCreate,
    ScaleSetFound, SessionError, WireError, admin_token_is_fresh, create_runner_scale_set,
    get_runner_scale_set, get_runner_scale_set_by_id, http_create_body, product_create_labels,
};

use regwire::{Script, header, show};

const NAME: &str = "ubuntu-26.04-scale-set";
const ADMIN: &str = "admin-secret-canary";

fn labels() -> String {
    r#"[{"name":"velnor","type":"System"},{"name":"ubuntu-26.04-scale-set","type":"System"}]"#
        .to_owned()
}

fn one_set(id: i64) -> String {
    format!(
        r#"{{"id":{id},"name":"{NAME}","labels":{},"RunnerSetting":{{"disableUpdate":true}}}}"#,
        labels(),
    )
}

fn page(count: i64, value: &str) -> String {
    format!(r#"{{"count":{count},"value":{value}}}"#)
}

#[test]
fn get_count_distinguishes_missing_duplicate_and_malformed() -> Result<(), String> {
    let mut missing = Script::once(200, &page(0, "[]"));
    let found = show(get_runner_scale_set(
        &mut missing,
        &ScaleSetByName {
            runner_group_id: 4,
            name: NAME,
            admin_token: ADMIN,
        },
    ))?;
    assert_eq!(found, ScaleSetFound::NotFound);
    let request = missing.seen.first().ok_or("request")?;
    assert_eq!(request.method, Method::Get);
    assert_eq!(request.path, "_apis/runtime/runnerscalesets");
    assert_eq!(
        request.query.as_deref(),
        Some("api-version=6.0-preview&name=ubuntu-26.04-scale-set&runnerGroupId=4")
    );
    assert!(header(request, CAPACITY_HEADER).is_none());
    let bearer = format!("Bearer {ADMIN}");
    assert_eq!(header(request, "Authorization"), Some(bearer.as_str()));
    assert!(!format!("{request:?}").contains(ADMIN));

    let duplicate = format!("[{},{}]", one_set(1), one_set(2));
    let mut many = Script::once(200, &page(2, &duplicate));
    let err = get_runner_scale_set(
        &mut many,
        &ScaleSetByName {
            runner_group_id: 4,
            name: NAME,
            admin_token: ADMIN,
        },
    );
    assert_eq!(
        err,
        Err(SessionError::Wire(WireError::RegistrationRejected))
    );

    let mut mismatch = Script::once(200, &page(1, "[]"));
    let err = get_runner_scale_set(
        &mut mismatch,
        &ScaleSetByName {
            runner_group_id: 4,
            name: NAME,
            admin_token: ADMIN,
        },
    );
    assert_eq!(err, Err(SessionError::Wire(WireError::Malformed)));
    Ok(())
}

#[test]
fn get_query_percent_encodes_reserved_bytes() -> Result<(), String> {
    let mut script = Script::once(200, &page(0, "[]"));
    let found = show(get_runner_scale_set(
        &mut script,
        &ScaleSetByName {
            runner_group_id: 3,
            name: "a b/c",
            admin_token: ADMIN,
        },
    ))?;
    assert_eq!(found, ScaleSetFound::NotFound);
    let request = script.seen.first().ok_or("request")?;
    assert_eq!(
        request.query.as_deref(),
        Some("api-version=6.0-preview&name=a+b%2Fc&runnerGroupId=3")
    );
    Ok(())
}

#[test]
fn create_sends_product_labels_and_preview_query() -> Result<(), String> {
    let labels = product_create_labels();
    let mut script = Script::once(200, &one_set(7));
    let created = show(create_runner_scale_set(
        &mut script,
        &ScaleSetCreate {
            name: NAME,
            runner_group_id: 1,
            labels: &labels,
            admin_token: ADMIN,
        },
    ))?;
    assert_eq!(created.id, 7);
    let request = script.seen.first().ok_or("request")?;
    assert_eq!(request.method, Method::Post);
    assert_eq!(request.query.as_deref(), Some("api-version=6.0-preview"));
    assert!(header(request, CAPACITY_HEADER).is_none());
    let body = String::from_utf8(request.body.clone()).map_err(|_| "utf8".to_owned())?;
    assert_eq!(
        body,
        r#"{"name":"ubuntu-26.04-scale-set","runnerGroupId":1,"labels":[{"type":"System","name":"velnor"},{"type":"System","name":"ubuntu-26.04-scale-set"}],"RunnerSetting":{"disableUpdate":true}}"#
    );
    let preview = http_create_body(NAME).map_err(|err| format!("{err:?}"))?;
    assert!(!preview.contains("runnerGroupId"));
    assert!(preview.contains("disableUpdate"));
    let parsed: serde_json::Value = serde_json::from_str(&body).map_err(|_| "json".to_owned())?;
    let names: Vec<&str> = parsed["labels"]
        .as_array()
        .ok_or("labels")?
        .iter()
        .filter_map(|label| label["name"].as_str())
        .collect();
    assert_eq!(names, ["velnor", "ubuntu-26.04-scale-set"]);
    assert!(!names.contains(&"ubuntu-26.04"));
    Ok(())
}

#[test]
fn empty_create_labels_use_the_set_name() -> Result<(), String> {
    let mut script = Script::once(500, "");
    let err = create_runner_scale_set(
        &mut script,
        &ScaleSetCreate {
            name: "generic-linux-build",
            runner_group_id: 1,
            labels: &[],
            admin_token: ADMIN,
        },
    );
    assert_eq!(err, Err(SessionError::Wire(WireError::UnexpectedStatus)));
    let request = script.seen.first().ok_or("request")?;
    let body = String::from_utf8(request.body.clone()).map_err(|_| "utf8".to_owned())?;
    assert_eq!(
        body,
        r#"{"name":"generic-linux-build","runnerGroupId":1,"labels":[{"type":"System","name":"generic-linux-build"}],"RunnerSetting":{"disableUpdate":true}}"#
    );
    let mut refused = Script::once(200, &one_set(7));
    let err = create_runner_scale_set(
        &mut refused,
        &ScaleSetCreate {
            name: "",
            runner_group_id: 1,
            labels: &[],
            admin_token: ADMIN,
        },
    );
    assert_eq!(
        err,
        Err(SessionError::Wire(WireError::RegistrationRejected))
    );
    assert_eq!(refused.seen.len(), 0);
    Ok(())
}

#[test]
fn product_names_require_exact_labels_before_transport() {
    for name in ["ubuntu-24.04-scale-set", NAME] {
        let no_labels: [CreateLabel; 0] = [];
        let vel_labels = [CreateLabel {
            name: "velnor".to_owned(),
            label_type: "System".to_owned(),
        }];
        let selector_labels = [CreateLabel {
            name: name.to_owned(),
            label_type: "System".to_owned(),
        }];

        for labels in [&no_labels[..], &vel_labels[..], &selector_labels[..]] {
            let mut script = Script::once(200, "{}");
            let result = create_runner_scale_set(
                &mut script,
                &ScaleSetCreate {
                    name,
                    runner_group_id: 1,
                    labels,
                    admin_token: ADMIN,
                },
            );
            assert_eq!(
                result,
                Err(SessionError::Wire(WireError::RegistrationRejected)),
                "product selector {name} with {} labels must fail before POST",
                labels.len()
            );
            assert!(script.seen.is_empty(), "transport called for {name}");
        }
    }
}

#[test]
fn unsupported_scale_set_name_with_empty_labels_fails_before_transport() {
    let mut script = Script::once(200, "{}");
    let result = create_runner_scale_set(
        &mut script,
        &ScaleSetCreate {
            name: "ubuntu-25.04-scale-set",
            runner_group_id: 1,
            labels: &[],
            admin_token: ADMIN,
        },
    );
    assert_eq!(
        result,
        Err(SessionError::Wire(WireError::RegistrationRejected))
    );
    assert_eq!(script.seen.len(), 0);
}

#[test]
fn get_by_id_decodes_and_accepts() -> Result<(), String> {
    let mut script = Script::once(200, &one_set(9));
    let view = show(get_runner_scale_set_by_id(
        &mut script,
        &ScaleSetById {
            scale_set_id: 9,
            name: NAME,
            admin_token: ADMIN,
        },
    ))?;
    assert_eq!(view.id, 9);
    let request = script.seen.first().ok_or("request")?;
    assert_eq!(request.path, "_apis/runtime/runnerscalesets/9");
    assert_eq!(request.query.as_deref(), Some("api-version=6.0-preview"));
    assert!(header(request, CAPACITY_HEADER).is_none());
    Ok(())
}

#[test]
fn admin_token_freshness_is_strict_past_sixty_seconds() {
    let now = 1_700_000_000_i64;
    assert!(!admin_token_is_fresh(now, 0));
    assert!(!admin_token_is_fresh(now, now));
    assert!(!admin_token_is_fresh(now, now + 59));
    assert!(!admin_token_is_fresh(now, now + 60));
    assert!(admin_token_is_fresh(now, now + 61));
}
