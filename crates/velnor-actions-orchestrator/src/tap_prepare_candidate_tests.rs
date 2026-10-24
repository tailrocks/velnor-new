use super::*;
use velnor_actions_contract::{
    homebrew_spec::{HomebrewFormulaReleaseLiterals, render_homebrew_formula},
    package_release::PackageReleaseChannel,
};

fn preview_release_at_count(
    fixture: &crate::homebrew::test_support::TapFixture,
    count: u64,
) -> VerifiedPackageRelease {
    let mut receipt = fixture.release.receipt.clone();
    receipt.version.commit_count = Some(count);
    receipt.version.source_sha7 = Some(receipt.source.commit_sha[..7].to_owned());
    receipt.version.value = format!(
        "{}-preview.{count}+{}",
        receipt.version.base,
        &receipt.source.commit_sha[..7]
    );
    receipt.validate().expect("adjusted preview receipt");
    VerifiedPackageRelease {
        receipt_digest: receipt.canonical_digest().expect("receipt digest"),
        receipt,
    }
}

fn stable_release(
    fixture: &crate::homebrew::test_support::TapFixture,
    version: &str,
    source_sha: &str,
) -> VerifiedPackageRelease {
    let mut receipt = fixture.release.receipt.clone();
    receipt.channel = PackageReleaseChannel::Stable;
    receipt.source.commit_sha = source_sha.to_owned();
    receipt.source.event =
        velnor_actions_contract::package_release::PackageReleaseEvent::WorkflowDispatch;
    receipt.workflow.authority_sha = source_sha.to_owned();
    receipt.workflow.path = PackageReleaseChannel::Stable.workflow_path().to_owned();
    receipt.workflow.signer_job_id = PackageReleaseChannel::Stable.signer_job_id().to_owned();
    receipt.workflow.signer_workflow = PackageReleaseChannel::Stable.workflow_path().to_owned();
    receipt.version.base = version.to_owned();
    receipt.version.value = version.to_owned();
    for product in &mut receipt.products {
        product.package_version = version.to_owned();
    }
    receipt.version.commit_count = None;
    receipt.version.source_sha7 = None;
    receipt.tag = format!("v{version}");
    receipt.evidence.signer_workflow_ref = format!(
        "{}/{}@{}",
        receipt.repository.slug, receipt.workflow.path, receipt.workflow.workflow_ref
    );
    receipt.validate().expect("stable receipt");
    VerifiedPackageRelease {
        receipt_digest: receipt.canonical_digest().expect("receipt digest"),
        receipt,
    }
}

fn rendered_formula(
    spec: &HomebrewFormulaSpec,
    plan: &PackageReleasePlan,
    release: &VerifiedPackageRelease,
) -> Vec<u8> {
    let projection = validate_homebrew_projection(spec, plan).expect("projection");
    let asset_digests = projection
        .selected_payloads()
        .iter()
        .map(|payload| {
            let asset = release
                .receipt
                .assets
                .iter()
                .find(|asset| asset.name == payload.asset_name())
                .expect("receipt payload asset");
            (asset.name.clone(), asset.sha256.clone())
        })
        .collect();
    let literals = HomebrewFormulaReleaseLiterals::new(
        spec,
        &projection,
        release.receipt.source.commit_sha.clone(),
        release.receipt.version.value.clone(),
        release.receipt.tag.clone(),
        asset_digests,
    )
    .expect("release literals");
    render_homebrew_formula(spec, &literals).expect("formula")
}

fn with_custom_install_test_and_caveat(formula: &[u8]) -> Vec<u8> {
    let formula = std::str::from_utf8(formula).expect("formula UTF-8");
    let end = formula.rfind("end\n").expect("formula class terminator");
    format!(
        "{}  depends_on \"ruby\"\n  def install\n    bin.install \"cli\"\n  end\n  test do\n    system \"true\"\n  end\n  # locally maintained caveat\n{}",
        &formula[..end],
        &formula[end..]
    )
    .into_bytes()
}

fn derive_for(
    fixture: &crate::homebrew::test_support::TapFixture,
    base: &[u8],
    plan: &PackageReleasePlan,
    release: &VerifiedPackageRelease,
) -> Result<Vec<u8>, OrchestratorError> {
    derive_native_formula_candidate(FormulaCandidateInput {
        spec: Some(&fixture.spec),
        source_repository: &fixture.plan.repository,
        tap_repository: &fixture.policy.repository,
        formula_path: &fixture.formula_path,
        base_sha: &"c".repeat(40),
        base_formula: base,
        proposed_formula: None,
        plan,
        release,
    })
}

#[test]
fn source_qualified_custom_route_preserves_install_test_and_caveat_lines() {
    let fixture = crate::homebrew::test_support::fixture();
    let base = with_custom_install_test_and_caveat(&fixture.base_formula);
    let proposed = with_custom_install_test_and_caveat(&fixture.proposed_formula);
    let candidate = derive_formula_candidate(FormulaCandidateInput {
        spec: None,
        source_repository: &fixture.plan.repository,
        tap_repository: &fixture.policy.repository,
        formula_path: &fixture.formula_path,
        base_sha: &"b".repeat(40),
        base_formula: &base,
        proposed_formula: Some(&proposed),
        plan: &fixture.plan,
        release: &fixture.release,
    })
    .expect("validate custom formula literals");
    assert_eq!(candidate, proposed);

    let proof = candidate_proof(&CandidateIdentity {
        source_repository: &fixture.plan.repository,
        tap: &fixture.policy,
        release: &fixture.release,
        spec: None,
        sdk_artifact_digest: &"9".repeat(64),
        base_sha: &"b".repeat(40),
        old_sha: &sha256(&base),
        formula: &candidate,
        plan: &fixture.plan,
        formula_path: &fixture.formula_path,
        disposition: TapPreparationDisposition::Changed,
    })
    .expect("custom candidate proof");
    assert_eq!(proof.formula_spec_digest, None);
    let encoded = canonical_json_bytes(&proof).expect("custom proof JSON");
    assert!(
        !encoded
            .windows(b"formula_spec_digest".len())
            .any(|part| { part == b"formula_spec_digest" })
    );
}

#[test]
fn native_formula_derivation_rejects_structural_base_changes_even_for_current_release() {
    let fixture = crate::homebrew::test_support::fixture();
    let canonical = derive_native_formula_candidate(FormulaCandidateInput {
        spec: Some(&fixture.spec),
        source_repository: &fixture.plan.repository,
        tap_repository: &fixture.policy.repository,
        formula_path: &fixture.formula_path,
        base_sha: &"b".repeat(40),
        base_formula: &fixture.base_formula,
        proposed_formula: None,
        plan: &fixture.plan,
        release: &fixture.release,
    })
    .expect("derive canonical update");
    assert_eq!(canonical, fixture.proposed_formula);
    assert!(
        derive_native_formula_candidate(FormulaCandidateInput {
            spec: Some(&fixture.spec),
            source_repository: &fixture.plan.repository,
            tap_repository: &fixture.policy.repository,
            formula_path: &fixture.formula_path,
            base_sha: &"b".repeat(40),
            base_formula: b"class CliPreview < Formula\n  version \"0.0.0\"\nend\n",
            proposed_formula: None,
            plan: &fixture.plan,
            release: &fixture.release,
        })
        .is_err(),
        "malformed structural base cannot become a no-op"
    );
}

#[test]
fn native_formula_derivation_rejects_preview_count_downgrade_and_identity_conflict() {
    let fixture = crate::homebrew::test_support::fixture();
    let lower = preview_release_at_count(&fixture, 7);
    assert!(derive_for(&fixture, &fixture.base_formula, &fixture.plan, &lower).is_err());

    let equal_count = preview_release_at_count(&fixture, 8);
    assert!(
        derive_for(&fixture, &fixture.base_formula, &fixture.plan, &equal_count).is_err(),
        "same preview count with another source identity is rejected"
    );
}

#[test]
fn native_formula_derivation_rejects_stable_downgrade_and_same_version_identity_conflict() {
    let fixture = crate::homebrew::test_support::fixture();
    let mut stable_plan = fixture.plan.clone();
    stable_plan.channel = PackageReleaseChannel::Stable;
    let stable_spec = fixture.spec.clone();
    let target = stable_release(&fixture, "1.2.3", &"a".repeat(40));
    let mut base_plan = stable_plan.clone();
    base_plan.base_version = "1.2.4".to_owned();
    let older_base_release = stable_release(&fixture, "1.2.4", &"b".repeat(40));
    let newer_base = rendered_formula(&stable_spec, &base_plan, &older_base_release);
    assert!(
        derive_native_formula_candidate(FormulaCandidateInput {
            spec: Some(&stable_spec),
            source_repository: &stable_plan.repository,
            tap_repository: &fixture.policy.repository,
            formula_path: "Formula/cli.rb",
            base_sha: &"c".repeat(40),
            base_formula: &newer_base,
            proposed_formula: None,
            plan: &stable_plan,
            release: &target,
        })
        .is_err(),
        "stable downgrade is rejected by native derivation"
    );

    let same_version_base_release = stable_release(&fixture, "1.2.3", &"b".repeat(40));
    let same_version_base =
        rendered_formula(&stable_spec, &stable_plan, &same_version_base_release);
    assert!(
        derive_native_formula_candidate(FormulaCandidateInput {
            spec: Some(&stable_spec),
            source_repository: &stable_plan.repository,
            tap_repository: &fixture.policy.repository,
            formula_path: "Formula/cli.rb",
            base_sha: &"c".repeat(40),
            base_formula: &same_version_base,
            proposed_formula: None,
            plan: &stable_plan,
            release: &target,
        })
        .is_err(),
        "same stable version with another source identity is rejected"
    );
}

#[test]
fn candidate_directory_appears_only_after_both_files_are_written() {
    let fixture = crate::homebrew::test_support::fixture();
    let temp = tempfile::tempdir().expect("temp");
    let proof = persist_candidate(
        temp.path(),
        CandidateIdentity {
            source_repository: &fixture.plan.repository,
            tap: &fixture.policy,
            release: &fixture.release,
            spec: Some(&fixture.spec),
            sdk_artifact_digest: &"9".repeat(64),
            base_sha: &"b".repeat(40),
            old_sha: &sha256(&fixture.base_formula),
            formula: &fixture.proposed_formula,
            plan: &fixture.plan,
            formula_path: &fixture.formula_path,
            disposition: TapPreparationDisposition::Changed,
        },
    )
    .expect("persist candidate");
    let directory = temp.path().join(CANDIDATE_DIRECTORY);
    assert!(directory.join("candidate.json").is_file());
    assert_eq!(
        fs::read(directory.join("formula.rb")).expect("formula bytes"),
        fixture.proposed_formula
    );
    assert_eq!(
        fs::read(directory.join("candidate.json")).expect("proof bytes"),
        canonical_json_bytes(&proof).expect("canonical proof")
    );
    assert!(
        persist_candidate(
            temp.path(),
            CandidateIdentity {
                source_repository: &fixture.plan.repository,
                tap: &fixture.policy,
                release: &fixture.release,
                spec: Some(&fixture.spec),
                sdk_artifact_digest: &"9".repeat(64),
                base_sha: &"b".repeat(40),
                old_sha: &sha256(&fixture.base_formula),
                formula: &fixture.proposed_formula,
                plan: &fixture.plan,
                formula_path: &fixture.formula_path,
                disposition: TapPreparationDisposition::Changed,
            },
        )
        .is_err(),
        "existing candidate output cannot be reused"
    );
}

#[test]
fn incomplete_staging_directory_blocks_candidate_retry() {
    let fixture = crate::homebrew::test_support::fixture();
    let temp = tempfile::tempdir().expect("temp");
    let staging = temp.path().join(".tap-candidate-interrupted");
    fs::create_dir(&staging).expect("interrupted stage");
    assert!(
        persist_candidate(
            temp.path(),
            CandidateIdentity {
                source_repository: &fixture.plan.repository,
                tap: &fixture.policy,
                release: &fixture.release,
                spec: Some(&fixture.spec),
                sdk_artifact_digest: &"9".repeat(64),
                base_sha: &"b".repeat(40),
                old_sha: &sha256(&fixture.base_formula),
                formula: &fixture.proposed_formula,
                plan: &fixture.plan,
                formula_path: &fixture.formula_path,
                disposition: TapPreparationDisposition::Changed,
            },
        )
        .is_err()
    );
    assert!(staging.is_dir(), "foreign incomplete stage stays untouched");
    assert!(!temp.path().join(CANDIDATE_DIRECTORY).exists());
}
