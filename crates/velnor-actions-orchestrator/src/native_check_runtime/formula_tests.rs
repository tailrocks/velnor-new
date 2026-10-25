use super::{
    validate_candidate_upgrade, verify_stable_sentinel, verify_stable_sentinel_transition,
};
use crate::{homebrew::test_support as tap_test_support, tap_prepare::FormulaCandidateInput};
use velnor_actions_contract::{
    VerifiedPackageRelease,
    homebrew_spec::{
        HomebrewFormulaReleaseLiterals, render_homebrew_formula, validate_homebrew_projection,
    },
    package_release::{PackageReleaseChannel, PackageReleaseEvent},
};

#[test]
fn protected_preview_formula_must_not_be_newer_than_candidate() {
    let fixture = tap_test_support::fixture();
    let projection =
        validate_homebrew_projection(&fixture.spec, &fixture.plan).expect("preview projection");
    let protected_base = preview_formula(&fixture, &projection, 10, "ccccccc");
    let result = validate_candidate_upgrade(
        FormulaCandidateInput {
            spec: Some(&fixture.spec),
            source_repository: "owner/source",
            tap_repository: "owner/homebrew-tap",
            formula_path: &fixture.formula_path,
            base_sha: "1111111111111111111111111111111111111111",
            base_formula: &protected_base,
            proposed_formula: None,
            plan: &fixture.plan,
            release: &fixture.release,
        },
        &fixture.proposed_formula,
    );
    assert!(
        result.is_err(),
        "candidate must not downgrade protected base"
    );
}

#[test]
fn protected_preview_formula_accepts_exact_retry() {
    let fixture = tap_test_support::fixture();
    let result = validate_candidate_upgrade(
        FormulaCandidateInput {
            spec: Some(&fixture.spec),
            source_repository: "owner/source",
            tap_repository: "owner/homebrew-tap",
            formula_path: &fixture.formula_path,
            base_sha: "1111111111111111111111111111111111111111",
            base_formula: &fixture.proposed_formula,
            proposed_formula: None,
            plan: &fixture.plan,
            release: &fixture.release,
        },
        &fixture.proposed_formula,
    );
    assert!(
        result.is_ok(),
        "identical authenticated release is a valid retry"
    );
}

#[test]
fn disabled_stable_formula_can_only_remain_disabled_from_disabled_base() {
    let fixture = tap_test_support::fixture();
    let mut stable_plan = fixture.plan.clone();
    stable_plan.channel = PackageReleaseChannel::Stable;
    let projection =
        validate_homebrew_projection(&fixture.spec, &stable_plan).expect("stable projection");
    let sentinel = HomebrewFormulaReleaseLiterals::stable_sentinel(&fixture.spec, &projection)
        .expect("stable sentinel");
    let sentinel_bytes =
        render_homebrew_formula(&fixture.spec, &sentinel).expect("stable sentinel formula");
    assert!(
        verify_stable_sentinel_transition(
            &fixture.spec,
            &projection,
            &sentinel_bytes,
            &sentinel_bytes,
        )
        .is_ok()
    );

    let active = stable_formula(&fixture, &stable_plan, "1.2.3", &"a".repeat(40));
    assert!(
        verify_stable_sentinel_transition(&fixture.spec, &projection, &sentinel_bytes, &active,)
            .is_err()
    );
}

#[test]
fn disabled_stable_formula_can_activate_at_first_stable_release() {
    let fixture = tap_test_support::fixture();
    let mut stable_plan = fixture.plan.clone();
    stable_plan.channel = PackageReleaseChannel::Stable;
    let projection =
        validate_homebrew_projection(&fixture.spec, &stable_plan).expect("stable projection");
    let sentinel = HomebrewFormulaReleaseLiterals::stable_sentinel(&fixture.spec, &projection)
        .expect("stable sentinel");
    let sentinel_bytes =
        render_homebrew_formula(&fixture.spec, &sentinel).expect("stable sentinel formula");
    let release = stable_release(&fixture, "1.2.3", &"a".repeat(40));
    let hashes = release
        .receipt
        .assets
        .iter()
        .filter(|asset| {
            asset.kind == velnor_actions_contract::package_release::PackageReleaseAssetKind::Payload
        })
        .map(|asset| (asset.name.clone(), asset.sha256.clone()))
        .collect();
    let literals = HomebrewFormulaReleaseLiterals::new(
        &fixture.spec,
        &projection,
        release.receipt.source.commit_sha.clone(),
        release.receipt.version.value.clone(),
        release.receipt.tag.clone(),
        hashes,
    )
    .expect("stable release literals");
    let candidate =
        render_homebrew_formula(&fixture.spec, &literals).expect("active stable formula");
    assert!(
        validate_candidate_upgrade(
            FormulaCandidateInput {
                spec: Some(&fixture.spec),
                source_repository: "owner/source",
                tap_repository: "owner/homebrew-tap",
                formula_path: "Formula/cli.rb",
                base_sha: "1111111111111111111111111111111111111111",
                base_formula: &sentinel_bytes,
                proposed_formula: None,
                plan: &stable_plan,
                release: &release,
            },
            &candidate,
        )
        .is_ok()
    );
}

#[test]
fn pinned_seed_formula_bytes_are_distinct_from_the_canonical_stable_sentinel() {
    let spec_bytes = include_bytes!("../../../../.velnor/qualification/homebrew/formula-spec.json");
    let plan_bytes = include_bytes!("../../../../.velnor/qualification/homebrew/preview-plan.json");
    let spec: velnor_actions_contract::homebrew_spec::HomebrewFormulaSpec =
        serde_json::from_slice(spec_bytes).expect("Jackin formula spec");
    let plan: velnor_actions_contract::PackageReleasePlan =
        serde_json::from_slice(plan_bytes).expect("Jackin preview plan");
    let legacy_preview = include_bytes!("testdata/jackin-preview-seed.rb");
    let legacy_stable = include_bytes!("testdata/jackin-stable-seed.rb");

    assert_eq!(
        crate::release_policy_admission::git_blob_sha1(legacy_preview).expect("preview blob"),
        "c2814fe70420b1a5514fb68051ccb7c6f5ee3a2a"
    );
    assert_eq!(
        crate::release_policy_admission::git_blob_sha1(legacy_stable).expect("stable blob"),
        "53e4bee74ff88f6230eb7b61188fa089d8030083"
    );
    let preview = validate_homebrew_projection(&spec, &plan).expect("Jackin preview projection");
    assert!(
        std::str::from_utf8(legacy_preview)
            .expect("legacy preview formula is UTF-8")
            .starts_with("# source-sha: 41bad147b5ba9db09be99b07248531ce1bd867be\n"),
        "the pinned legacy formula retains its reviewed source header"
    );
    assert!(
        velnor_actions_contract::homebrew_spec::parse_generated_formula(
            &spec,
            &preview,
            legacy_preview,
        )
        .is_err(),
        "legacy seed bytes are intentionally not the current canonical formula template"
    );

    let mut stable_plan = plan;
    stable_plan.channel = PackageReleaseChannel::Stable;
    let stable = validate_homebrew_projection(&spec, &stable_plan).expect("stable projection");
    let sentinel = HomebrewFormulaReleaseLiterals::stable_sentinel(&spec, &stable)
        .expect("canonical disabled stable formula");
    let canonical_sentinel =
        render_homebrew_formula(&spec, &sentinel).expect("rendered stable sentinel");
    assert_ne!(legacy_stable.as_slice(), canonical_sentinel.as_slice());
    assert!(verify_stable_sentinel(&spec, &stable, legacy_stable).is_err());
    assert!(verify_stable_sentinel(&spec, &stable, &canonical_sentinel).is_ok());
}

fn preview_formula(
    fixture: &tap_test_support::TapFixture,
    projection: &velnor_actions_contract::homebrew_spec::ValidatedHomebrewProjection,
    count: u64,
    source_prefix: &str,
) -> Vec<u8> {
    assert_eq!(fixture.plan.channel, PackageReleaseChannel::Preview);
    let source = format!("{source_prefix}{}", "0".repeat(40 - source_prefix.len()));
    let version = format!("1.2.3-preview.{count}+{}", &source[..7]);
    let literals = HomebrewFormulaReleaseLiterals::new(
        &fixture.spec,
        projection,
        source.clone(),
        version,
        format!("preview-{source}"),
        projection
            .selected_payloads()
            .iter()
            .map(|payload| (payload.asset_name().to_owned(), "b".repeat(64)))
            .collect(),
    )
    .expect("preview literals");
    render_homebrew_formula(&fixture.spec, &literals).expect("preview formula")
}

fn stable_formula(
    fixture: &tap_test_support::TapFixture,
    plan: &velnor_actions_contract::PackageReleasePlan,
    version: &str,
    source_sha: &str,
) -> Vec<u8> {
    let projection = validate_homebrew_projection(&fixture.spec, plan).expect("projection");
    let release = stable_release(fixture, version, source_sha);
    let hashes = release
        .receipt
        .assets
        .iter()
        .filter(|asset| {
            asset.kind == velnor_actions_contract::package_release::PackageReleaseAssetKind::Payload
        })
        .map(|asset| (asset.name.clone(), asset.sha256.clone()))
        .collect();
    let literals = HomebrewFormulaReleaseLiterals::new(
        &fixture.spec,
        &projection,
        release.receipt.source.commit_sha,
        release.receipt.version.value,
        release.receipt.tag,
        hashes,
    )
    .expect("stable release literals");
    render_homebrew_formula(&fixture.spec, &literals).expect("stable formula")
}

fn stable_release(
    fixture: &tap_test_support::TapFixture,
    version: &str,
    source_sha: &str,
) -> VerifiedPackageRelease {
    let mut receipt = fixture.release.receipt.clone();
    receipt.channel = PackageReleaseChannel::Stable;
    receipt.source.commit_sha = source_sha.to_owned();
    receipt.source.event = PackageReleaseEvent::WorkflowDispatch;
    receipt.workflow.authority_sha = source_sha.to_owned();
    receipt.workflow.path = PackageReleaseChannel::Stable.workflow_path().to_owned();
    receipt.workflow.signer_job_id = PackageReleaseChannel::Stable.signer_job_id().to_owned();
    receipt.workflow.signer_workflow = PackageReleaseChannel::Stable.workflow_path().to_owned();
    receipt.version.base = version.to_owned();
    receipt.version.value = version.to_owned();
    receipt.version.commit_count = None;
    receipt.version.source_sha7 = None;
    receipt.tag = format!("v{version}");
    for product in &mut receipt.products {
        product.package_version = version.to_owned();
    }
    receipt.evidence.signer_workflow_ref = format!(
        "{}/{}@{}",
        receipt.repository.slug, receipt.workflow.path, receipt.workflow.workflow_ref
    );
    receipt.validate().expect("stable test receipt");
    VerifiedPackageRelease {
        receipt_digest: receipt.canonical_digest().expect("stable receipt digest"),
        receipt,
    }
}
