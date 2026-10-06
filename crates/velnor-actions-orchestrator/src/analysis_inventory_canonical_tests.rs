//! Independent canonical wire facts: no DTO serializer/decoder supplies expected metadata.

use serde_json::{Value, json};
use velnor_actions_contract::canonical_json_bytes;

const MEMBER_ID: &str = "path+file://velnor-checkout/member#member@1.0.0";

#[test]
fn normalized_payload_matches_independent_complete_canonical_fixture() {
    let root = tempfile::tempdir().expect("root");
    let (files, records) = super::fixture(root.path());
    let digest = super::resolution_inputs_digest(root.path(), &files, &records).expect("inputs");
    let payload = super::build_payload(root.path(), super::identity(digest.clone()), &records)
        .expect("payload");
    let expected = json!({
        "schema": 1,
        "identity": {
            "helper_sha256": "a".repeat(64),
            "cargo_identity": "cargo 1.98.1 (797e8a9bc 2026-08-05)",
            "cargo_pin": "1.98.1",
            "resolution_inputs_digest": digest,
            "source": {
                "repository": "owner/repo",
                "head_sha": "b".repeat(40),
                "workflow_sha": "c".repeat(40),
                "run_id": 42,
                "run_attempt": 2,
                "branch": "main"
            }
        },
        "inventories": [["Cargo.toml", expected_workspace()]]
    });
    let expected_bytes = canonical_json_bytes(&expected).expect("independent canonical fixture");
    assert_eq!(payload.as_bytes(), expected_bytes.as_slice());
}

fn expected_workspace() -> Value {
    json!({
        "workspace_root": "",
        "members": [MEMBER_ID],
        "packages": [{
            "id": MEMBER_ID,
            "name": "member",
            "version": "1.0.0",
            "manifest": "member/Cargo.toml",
            "external": false,
            "in_workspace": true,
            "targets": [{
                "kind": "proc-macro",
                "name": "member",
                "test": true,
                "doctest": false,
                "required_features": ["feature"]
            }],
            "features": ["feature"],
            "has_build_script": true
        }],
        "edges": [expected_edge("Normal"), expected_edge("Build"), expected_edge("Dev")],
        "skipped_edges": [{
            "from": MEMBER_ID,
            "path": "velnor-checkout:other",
            "kind": "Dev",
            "optional": true,
            "target": "cfg(windows)"
        }]
    })
}

fn expected_edge(kind: &str) -> Value {
    json!({
        "from": MEMBER_ID,
        "to": MEMBER_ID,
        "kind": kind,
        "optional": true,
        "target": "cfg(unix)"
    })
}
