//! Exercise the exact included source parser, never a second report interpreter.
use serde_json::{Value, json};
use std::io::Write;
use std::process::{Command, Stdio};

fn report(emitted: bool) -> Value {
    json!({
        "version": 2, "budget_refused": false, "snapshot_budget_bytes": 4096,
        "exported": emitted, "actions": 1, "objects": 1, "bytes": 12,
        "emitted_bundle_useful_delta": emitted,
        "workspace_usefulness": {"status": "unavailable", "reason": "scheduler_validity_not_proven"},
        "delta": {"new_action_results": u64::from(emitted), "changed_action_results": 0,
            "new_predictions": 0, "new_workspace_variants": 0, "changed_workspace_variants": 0},
        "semantic_digest": "a".repeat(64),
        "workspace_comparison": "relative_path_type_content_mode_symlink_target",
        "workspace_comparison_exclusions": ["effective_build_root/.rustc_info.json"],
        "workspace_transport_scope": "recorded_target_and_build_directories",
        "workspace_capture": "captured", "workspace_capture_unavailable_reason": null,
        "workspace_persistence_verified": false, "qualification": "source-only fixture"
    })
}

fn parse_text(input: &str) -> std::process::Output {
    let script = format!(
        "import json,re,sys\n{}\nprint(json.dumps(export_report(decode_native_report(sys.stdin.read()))))\n",
        include_str!("mbx_export_report.py")
    );
    let mut child = Command::new("/usr/bin/python3")
        .args(["-I", "-S", "-c", &script])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("same fixed source parser runtime");
    child
        .stdin
        .take()
        .expect("parser input")
        .write_all(input.as_bytes())
        .expect("write fixture");
    child.wait_with_output().expect("source parser outcome")
}

#[test]
fn bundle_delta_never_becomes_a_workspace_usefulness_claim() {
    for emitted in [false, true] {
        let output = parse_text(&report(emitted).to_string());
        assert!(output.status.success(), "{:?}", output);
        let parsed: Value = serde_json::from_slice(&output.stdout).expect("parser output");
        assert_eq!(
            parsed,
            json!([emitted, "a".repeat(64), "scheduler_validity_not_proven"])
        );
    }
    for (capture, reason) in [
        ("unavailable_owner_coverage", "owner_coverage_unavailable"),
        ("unavailable_owner_proof", "owner_proof_unavailable"),
        ("unavailable_managed_overlap", "managed_overlap"),
    ] {
        let mut value = report(true);
        value["workspace_capture"] = json!(capture);
        value["workspace_usefulness"]["reason"] = json!(reason);
        value["workspace_capture_unavailable_reason"] = json!("owner explanation");
        assert!(parse_text(&value.to_string()).status.success());
    }
}

#[test]
fn legacy_aliases_and_unproven_workspace_claims_are_rejected() {
    let cases = [
        ("version", json!(1)),
        ("version", json!(true)),
        ("version", json!(2.0)),
        ("useful_delta", json!(true)),
        ("unknown", json!(0)),
        ("workspace_usefulness", Value::Null),
        ("workspace_usefulness", json!(false)),
        (
            "workspace_usefulness",
            json!({"status":"identical","reason":"scheduler_validity_not_proven"}),
        ),
        (
            "workspace_usefulness",
            json!({"status":"useful","reason":"scheduler_validity_not_proven"}),
        ),
        (
            "workspace_usefulness",
            json!({"status":"unavailable","reason":"unknown"}),
        ),
        ("workspace_persistence_verified", json!(true)),
        ("workspace_persistence_verified", Value::Null),
        ("emitted_bundle_useful_delta", Value::Null),
        ("actions", json!(true)),
        ("delta", Value::Null),
        ("snapshot_budget_bytes", Value::Null),
    ];
    for (key, replacement) in cases {
        let mut value = report(true);
        value[key] = replacement;
        assert!(
            !parse_text(&value.to_string()).status.success(),
            "accepted {key}: {value}"
        );
    }
    let duplicate = format!("{{\"version\":2,{}", &report(true).to_string()[1..]);
    assert!(!parse_text(&duplicate).status.success());
}
