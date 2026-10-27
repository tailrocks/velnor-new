// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{MARKER_LIMIT, parse_marker};
use velnor_actions_contract::canonical_json_bytes;

fn passive_marker() -> serde_json::Value {
    serde_json::json!({
        "schema": "velnor.homebrew-cutover-initialization.v1",
        "seed": {
            "schema": "homebrew-seed-output-base-v1",
            "transition_id": "passive-transition",
            "transition_digest": "passive-transition-digest",
            "transition_base_sha": "passive-transition-base",
            "transition_base_tree": "passive-transition-tree",
            "seed_output_base_sha": "passive-seed-base",
            "seed_output_base_tree": "passive-seed-tree",
            "sdk_source_sha": "passive-sdk-source",
            "sdk_version": "0.1.57",
            "sdk_manifest_sha256": "passive-sdk-manifest",
            "sdk_seed_runner_binary_sha256": "passive-sdk-runner",
            "formula_spec_digest": "passive-spec",
            "witness_digest": "passive-witness"
        },
        "source_sha": "passive-source",
        "locator_blob_sha": "passive-locator",
        "preview_receipt_digest": "passive-receipt",
        "preview_formula_sha256": "passive-formula"
    })
}

#[test]
fn cutover_marker_parser_roundtrips_canonical_passive_data() {
    let bytes = canonical_json_bytes(&passive_marker()).expect("canonical passive data");
    let parsed = parse_marker(&bytes).expect("passive marker parses");
    assert_eq!(
        canonical_json_bytes(&parsed).expect("canonical parsed data"),
        bytes
    );
}

#[test]
fn cutover_marker_parser_rejects_invalid_encodings() {
    let canonical = canonical_json_bytes(&passive_marker()).expect("canonical passive data");
    let mut unknown = passive_marker();
    unknown["unexpected"] = serde_json::json!(true);
    let mut schema = passive_marker();
    schema["schema"] = serde_json::json!("unsupported");
    let mut oversized = passive_marker();
    oversized["source_sha"] = serde_json::json!("x".repeat(MARKER_LIMIT));
    let duplicate = [
        b"{\"schema\":\"velnor.homebrew-cutover-initialization.v1\",".as_slice(),
        &canonical[1..],
    ]
    .concat();
    let noncanonical = [canonical.as_slice(), b"\n"].concat();
    for (case, bytes) in [
        ("duplicate key", duplicate),
        (
            "unknown field",
            canonical_json_bytes(&unknown).expect("unknown passive data"),
        ),
        ("noncanonical encoding", noncanonical),
        ("malformed JSON", b"{".to_vec()),
        (
            "size cap",
            canonical_json_bytes(&oversized).expect("oversized passive data"),
        ),
        (
            "unsupported schema",
            canonical_json_bytes(&schema).expect("unsupported passive data"),
        ),
    ] {
        assert!(parse_marker(&bytes).is_err(), "{case}");
    }
}
