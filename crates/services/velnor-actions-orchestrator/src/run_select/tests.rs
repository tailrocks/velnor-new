use super::*;

/// Only an explicit `"expired": false` selects; absent, null, true,
/// and non-bool markers never do.
#[test]
fn artifact_expiry_must_be_explicitly_false() {
    let listed = serde_json::json!({"artifacts": [
        {"id": 10, "name": "n", "expired": false},
    ]});
    assert_eq!(select_baseline_artifact(&listed.to_string(), "n"), Ok(10));
    for (label, entry) in [
        ("absent", serde_json::json!({"id": 11, "name": "n"})),
        (
            "null",
            serde_json::json!({"id": 12, "name": "n", "expired": null}),
        ),
        (
            "true",
            serde_json::json!({"id": 13, "name": "n", "expired": true}),
        ),
        (
            "string",
            serde_json::json!({"id": 14, "name": "n", "expired": "false"}),
        ),
        (
            "number",
            serde_json::json!({"id": 15, "name": "n", "expired": 0}),
        ),
    ] {
        let listed = serde_json::json!({"artifacts": [entry]});
        assert!(
            select_baseline_artifact(&listed.to_string(), "n").is_err(),
            "{label} expiry must never select"
        );
    }
}
