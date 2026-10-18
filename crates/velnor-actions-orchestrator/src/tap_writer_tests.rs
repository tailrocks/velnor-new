use super::*;

fn proof_json() -> serde_json::Value {
    serde_json::json!({
        "schema": "velnor.homebrew-candidate.v1",
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
