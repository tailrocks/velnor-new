use std::path::Path;

use serde_json::json;

use super::*;

#[test]
fn action_review_date_rejects_year_zero_and_accepts_year_one() {
    let mut invalid = FreshnessContext::new(Path::new("."), false, false);
    check_action_shape(
        &mut invalid,
        &json!({"name": "fixture", "reviewed": "0000-01-01"}),
    );
    assert!(
        invalid
            .failures
            .iter()
            .any(|failure| failure.contains("reviewed must be YYYY-MM-DD")),
        "year zero passed the action policy date parser"
    );

    let mut valid = FreshnessContext::new(Path::new("."), false, false);
    check_action_shape(
        &mut valid,
        &json!({"name": "fixture", "reviewed": "0001-01-01"}),
    );
    assert!(valid.failures.is_empty(), "{:?}", valid.failures);
}

#[test]
fn retired_tool_rows_are_outside_the_current_tool_set() {
    for (name, policy) in [
        ("reuse", json!({"tools": {"reuse": "6.1.0"}})),
        ("python", json!({"tools": {"python": "3.14.1"}})),
        ("uv", json!({"tools": {"uv": "0.9.0"}})),
    ] {
        let mut context = FreshnessContext::new(Path::new("."), false, false);
        context.policy = Some(policy);
        check_policy_mirror(&mut context);
        assert!(
            context.failures.iter().any(|failure| {
                failure.contains(&format!("tool {name}"))
                    && failure.contains("outside the expected tool set")
            }),
            "retired tool {name} unexpectedly passed: {:?}",
            context.failures
        );
    }
}
