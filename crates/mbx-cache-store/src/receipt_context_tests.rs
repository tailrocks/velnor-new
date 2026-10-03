use super::*;

fn context() -> ReceiptContext {
    ReceiptContext {
        schema: 1,
        source: serde_json::json!({"revision": "abc", "nested": {"z": 2, "a": 1}}),
        tool: serde_json::json!({"version": "1.21.0", "domain": "cargo"}),
    }
}

#[test]
fn canonical_context_round_trips() {
    let context = context();
    let text = context.canonical_json().unwrap();
    assert_eq!(ReceiptContext::from_canonical_json(&text).unwrap(), context);
    assert!(text.contains(r#""nested":{"a":1,"z":2}"#));
}

#[test]
fn refuses_noncanonical_encoding_and_duplicate_keys() {
    let text = context().canonical_json().unwrap();
    for broken in [
        format!(" {text}"),
        text.replace("abc", "\\u0061bc"),
        text.replace(r#""schema":1"#, r#""schema":1,"schema":1"#),
        text.replace(r#""a":1,"z":2"#, r#""z":2,"a":1"#),
    ] {
        assert!(
            ReceiptContext::from_canonical_json(&broken).is_err(),
            "{broken}"
        );
    }
}

#[test]
fn refuses_unknown_fields_schemas_missing_and_empty_claims() {
    let original = serde_json::to_value(context()).unwrap();
    for defect in ["schema", "source", "tool", "missing", "unknown", "array"] {
        let mut broken = original.clone();
        match defect {
            "schema" => broken["schema"] = 2.into(),
            "source" => broken["source"] = serde_json::json!({}),
            "tool" => broken["tool"] = serde_json::json!({}),
            "missing" => {
                broken.as_object_mut().unwrap().remove("source");
            }
            "unknown" => broken["extra"] = true.into(),
            "array" => broken["tool"] = serde_json::json!(["cargo"]),
            _ => unreachable!(),
        }
        assert!(ReceiptContext::from_canonical_json(&broken.to_string()).is_err());
    }
}

#[test]
fn refuses_oversized_context_before_parsing_or_encoding() {
    assert!(ReceiptContext::from_canonical_json(&" ".repeat(MAX_CONTEXT_BYTES + 1)).is_err());
    let mut context = context();
    context.source["large"] = "x".repeat(MAX_CONTEXT_BYTES).into();
    assert!(context.validate().is_err());
    assert!(context.canonical_json().is_err());
}
