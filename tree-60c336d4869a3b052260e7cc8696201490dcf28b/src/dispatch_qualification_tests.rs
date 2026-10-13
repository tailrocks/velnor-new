use super::qualification_lineage_required;

#[test]
fn only_predecessor_phases_load_an_admission_document() {
    for phase in ["warm", "third", "useful_delta"] {
        assert!(qualification_lineage_required(&response(phase)).is_ok_and(|value| value));
    }
    for phase in ["cold", "control"] {
        assert!(qualification_lineage_required(&response(phase)).is_ok_and(|value| !value));
    }
    assert!(qualification_lineage_required(r#"{"plan":{"qualification":null}}"#)
        .is_ok_and(|value| !value));
}

#[test]
fn malformed_plan_context_never_skips_or_loads_admission() {
    for response in [
        "not-json",
        r#"{"plan":{}}"#,
        r#"{"plan":{"qualification":{}}}"#,
        r#"{"plan":{"qualification":{"phase":"push"}}}"#,
    ] {
        assert!(qualification_lineage_required(response).is_err());
    }
}

fn response(phase: &str) -> String {
    serde_json::json!({"plan":{"qualification":{"phase":phase}}}).to_string()
}
