use super::*;

fn proof_json() -> serde_json::Value {
    serde_json::json!({
        "schema": "velnor.homebrew-candidate.v2",
        "disposition": "changed",
        "base_sha": "a".repeat(40),
        "formula_path": "Formula/example.rb",
        "old_formula_sha256": "b".repeat(64),
        "new_formula_sha256": "c".repeat(64),
        "receipt_digest": format!("b3-{}", "d".repeat(64)),
        "plan_digest": "e".repeat(64),
        "tap_config_digest": "f".repeat(64),
        "tap_repository": "owner/tap",
        "source_repository": "owner/source",
        "source_sha": "a".repeat(40),
        "version": "1.2.3-preview.1.gabcdef0",
        "candidate_formula": "formula.rb"
    })
}

#[test]
fn candidate_proof_rejects_unknown_fields() {
    let mut value = proof_json();
    value["writer_command"] = serde_json::json!("ruby -e unsafe");
    assert!(serde_json::from_value::<HomebrewTapPreparationProof>(value).is_err());
}

#[test]
fn candidate_proof_uses_fixed_relative_formula_name() {
    let proof: HomebrewTapPreparationProof = serde_json::from_value(proof_json()).expect("proof");
    assert_eq!(proof.candidate_formula, "formula.rb");
    assert_ne!(proof.candidate_formula, "/tmp/untrusted.rb");
}

#[test]
fn branch_name_binds_receipt_and_tap_base() {
    let receipt = format!("b3-{}", "d".repeat(64));
    let first = "b".repeat(40);
    let second = "c".repeat(40);
    assert_eq!(
        branch_name(&receipt, &first).expect("branch"),
        branch_name(&receipt, &first).expect("branch")
    );
    assert_ne!(
        branch_name(&receipt, &first).expect("branch"),
        branch_name(&receipt, &second).expect("branch")
    );
}

#[test]
fn digest_validator_requires_lowercase_sha256() {
    assert!(is_sha256(&"a".repeat(64)));
    assert!(!is_sha256(&"A".repeat(64)));
    assert!(!is_sha256(&"g".repeat(64)));
}

#[test]
fn promotion_outcome_v2_is_tagged_and_closed() {
    let value = serde_json::json!({
        "state": "already_promoted",
        "pull_request": 17,
        "head_sha": "a".repeat(40),
        "base_sha": "b".repeat(40),
        "branch": "velnor/preview/example",
        "candidate_proof_digest": "c".repeat(64),
        "merge_sha": "d".repeat(40),
        "merged_at": "2026-10-18T10:00:00Z"
    });
    let parsed: TapPromotionOutcome = serde_json::from_value(value.clone()).expect("outcome");
    assert!(matches!(
        parsed,
        TapPromotionOutcome::AlreadyPromoted { .. }
    ));

    let mut unknown = value;
    unknown["merge_authorized"] = serde_json::json!(true);
    assert!(serde_json::from_value::<TapPromotionOutcome>(unknown).is_err());
}

#[test]
fn pull_request_promotion_outcome_has_a_distinct_tagged_shape() {
    let outcome = TapPromotionOutcome::PullRequest {
        pull_request: 17,
        head_sha: "a".repeat(40),
        base_sha: "b".repeat(40),
        branch: format!("velnor/preview/{}-{}", "c".repeat(64), "b".repeat(40)),
        candidate_proof_digest: "d".repeat(64),
    };
    let value = serde_json::to_value(&outcome).expect("pull request outcome JSON");
    assert_eq!(value["state"], "pull_request");
    assert_eq!(value["pull_request"], 17);
    assert!(value.get("merge_sha").is_none());
    assert!(value.get("merged_at").is_none());
    assert!(serde_json::from_value::<TapPromotionOutcome>(value).is_ok());
}
