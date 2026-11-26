// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use anyhow::{Context, Result};
use serde_json::json;
use velnor_runner::context_value::ContextValue;
use velnor_runner::job_message::{AgentJobRequestMessage, MaskKind, template_token_context_value};

const FIXTURE: &str = include_str!("../../fixtures/job-message-wire.json");

#[test]
fn job_message_parses_wire_fixture() -> Result<()> {
    let message = AgentJobRequestMessage::parse_json(FIXTURE).context("parse fixture")?;

    assert_eq!(message.job_id, "00000000-0000-0000-0000-000000000001");
    assert_eq!(message.plan.plan_id, "00000000-0000-0000-0000-000000000002");
    assert_eq!(message.steps.len(), 1);
    assert_eq!(message.steps[0].script_text(), Some("echo hello"));
    assert_eq!(message.mask.len(), 1);
    assert_eq!(
        message.mask[0]
            .value
            .as_ref()
            .map(|value| value.expose().as_str()),
        Some("token-value")
    );

    let token = message
        .resources
        .endpoints
        .first()
        .and_then(|endpoint| endpoint.authorization.as_ref())
        .and_then(|authorization| authorization.parameter_string("accesstoken"));
    assert_eq!(token, Some("system-token"));

    let contexts = message
        .materialize_context_values()
        .context("materialize context values")?;
    let github = contexts.expose().get("github").context("github context")?;
    assert_eq!(
        github.get("repository"),
        Some(&ContextValue::String("tailrocks/velnor-new".to_owned()))
    );
    assert_eq!(
        github.get("run_attempt"),
        Some(&ContextValue::Number(1.into()))
    );
    let event = github.get("event").context("event context")?;
    assert_eq!(event.get("forced"), Some(&ContextValue::Bool(false)));
    assert_eq!(
        event.get("labels"),
        Some(&ContextValue::Array(vec![
            ContextValue::String("bug".to_owned()),
            ContextValue::Bool(true),
            ContextValue::Number(3.into()),
        ]))
    );
    Ok(())
}

fn materialize_one(context: &serde_json::Value) -> Result<ContextValue> {
    let message =
        AgentJobRequestMessage::from_value(json!({ "ContextData": { "value": context } }))
            .context("parse message")?;
    message
        .materialize_context_values()?
        .into_inner()
        .remove("value")
        .context("value context")
}

#[test]
fn context_data_reads_bare_scalars_like_upstream() -> Result<()> {
    assert_eq!(
        materialize_one(&json!("text"))?,
        ContextValue::String("text".to_owned())
    );
    assert_eq!(materialize_one(&json!(true))?, ContextValue::Bool(true));
    assert_eq!(
        materialize_one(&json!(2.5))?,
        ContextValue::Number(serde_json::Number::from_f64(2.5).context("finite")?)
    );
    assert_eq!(materialize_one(&json!(7))?, ContextValue::Number(7.into()));
    Ok(())
}

#[test]
fn context_data_reads_object_without_type_as_string() -> Result<()> {
    assert_eq!(
        materialize_one(&json!({"s": "text"}))?,
        ContextValue::String("text".to_owned())
    );
    assert_eq!(
        materialize_one(&json!({}))?,
        ContextValue::String(String::new())
    );
    Ok(())
}

#[test]
fn context_data_reads_non_integer_type_array_and_null_as_null() -> Result<()> {
    assert_eq!(materialize_one(&json!({"t": "2"}))?, ContextValue::Null);
    assert_eq!(materialize_one(&json!({"t": 2.0}))?, ContextValue::Null);
    assert_eq!(materialize_one(&json!({"t": null}))?, ContextValue::Null);
    assert_eq!(materialize_one(&json!([1, 2]))?, ContextValue::Null);
    assert_eq!(materialize_one(&json!(null))?, ContextValue::Null);
    Ok(())
}

#[test]
fn context_data_rejects_unknown_integer_type() {
    assert!(materialize_one(&json!({"t": 9})).is_err());
}

fn unsupported(spelling: &str) -> MaskKind {
    MaskKind::Unsupported(spelling.to_owned())
}

#[test]
fn mask_hints_accept_integers_numeric_strings_and_names() -> Result<()> {
    let mut wire: serde_json::Value = serde_json::from_str(FIXTURE)?;
    wire["Mask"] = json!([
        {"Type": 1, "Value": "first"},
        {"type": 2, "value": "second"},
        {"Type": "Variable", "Value": "third"},
        {"Type": "regex", "Value": "fourth"},
        {"Type": "1", "Value": "fifth"},
        {"Type": " 2 ", "Value": "sixth"},
    ]);

    let message = AgentJobRequestMessage::from_value(wire)?;

    let kinds: Vec<_> = message.mask.iter().map(|hint| hint.kind.clone()).collect();
    assert_eq!(
        kinds,
        [
            MaskKind::Variable,
            MaskKind::Regex,
            MaskKind::Variable,
            MaskKind::Regex,
            MaskKind::Variable,
            MaskKind::Regex,
        ]
    );
    Ok(())
}

#[test]
fn mask_hints_with_unknown_integer_or_missing_types_are_kept_for_the_worker_to_skip() -> Result<()>
{
    let mut wire: serde_json::Value = serde_json::from_str(FIXTURE)?;
    wire["Mask"] = json!([
        {"Type": 3, "Value": "integer"},
        {"Type": 0, "Value": "zero"},
        {"Type": "7", "Value": "numeric string"},
        {"Type": "Variable, Regex", "Value": "joined"},
        {"Type": " Regex ,Regex", "Value": "repeated"},
        {"Type": "Variable,1", "Value": "mixed"},
        {"Type": "-1", "Value": "negative"},
        {"Value": "absent"},
        {"Type": 1, "Value": "kept"},
    ]);

    let message = AgentJobRequestMessage::from_value(wire)?;

    let kinds: Vec<_> = message.mask.iter().map(|hint| hint.kind.clone()).collect();
    assert_eq!(
        kinds,
        [
            unsupported("3"),
            unsupported("0"),
            unsupported("7"),
            unsupported("3"),
            MaskKind::Regex,
            MaskKind::Variable,
            unsupported("-1"),
            unsupported("0"),
            MaskKind::Variable,
        ]
    );
    assert_eq!(
        message.mask[8]
            .value
            .as_ref()
            .map(|value| value.expose().as_str()),
        Some("kept")
    );
    assert_eq!(message.steps.len(), 1);
    Ok(())
}

#[test]
fn mask_hints_with_a_malformed_type_fail_the_job_message() -> Result<()> {
    for bad in [
        json!("Bogus"),
        json!(null),
        json!(1.5),
        json!(true),
        json!({"a": 1}),
        json!([1]),
        json!(2_147_483_648_i64),
        json!("2147483648"),
        json!("Variable,2147483648"),
        json!("Variable,Bogus"),
        json!(""),
        json!("Variable,"),
    ] {
        let mut wire: serde_json::Value = serde_json::from_str(FIXTURE)?;
        wire["Mask"] = json!([{"Type": bad, "Value": "secret"}]);
        assert!(
            AgentJobRequestMessage::from_value(wire).is_err(),
            "{bad} must fail closed"
        );
    }
    Ok(())
}

#[test]
fn insert_expression_reads_its_directive() -> Result<()> {
    let insert = json!({"type": 4, "directive": "insert"});
    assert_eq!(
        template_token_context_value(&insert)?,
        ContextValue::String("insert".to_owned())
    );

    let mapping = json!({
        "type": 2,
        "map": [{"key": insert, "value": {"type": 0, "lit": "payload"}}]
    });
    let expected = ContextValue::object(vec![(
        "insert".to_owned(),
        ContextValue::String("payload".to_owned()),
    )])
    .context("expected mapping")?;
    assert_eq!(template_token_context_value(&mapping)?, expected);
    Ok(())
}
