use super::super::tests::*;

fn assert_downgrade_rejected(
    spec: &HomebrewFormulaSpec,
    policy: &crate::config::RustBinaryReleaseConfig,
    channel: PackageReleaseChannel,
    formula_path: &str,
    old_bytes: &[u8],
) {
    let older_projection =
        validate_homebrew_projection(spec, &jackin_plan_from_policy(policy, channel, "1.2.2"))
            .expect("older projection");
    let older_release = next_verified_release(channel, "1.2.2");
    let older_literals = HomebrewFormulaReleaseLiterals::new(
        spec,
        &older_projection,
        older_release.receipt.source.commit_sha.clone(),
        older_release.receipt.version.value.clone(),
        older_release.receipt.tag.clone(),
        receipt_asset_hashes(&older_projection, &older_release),
    )
    .expect("older release literals");
    let older_bytes = render_homebrew_formula(spec, &older_literals).expect("older formula");
    let older_plan = jackin_plan_from_policy(policy, channel, "1.2.2");
    let downgrade = crate::homebrew_formula::HomebrewFormulaCandidate {
        source_repository: "jackin-project/jackin",
        plan: &older_plan,
        tap_repository: "jackin-project/homebrew-tap",
        formula_path,
        captured_base_sha: BASE_SHA,
        base_bytes: old_bytes,
        proposed_bytes: &older_bytes,
        release: &older_release,
    };
    assert!(
        crate::homebrew_formula::validate_homebrew_formula_candidate(&downgrade).is_err(),
        "{channel:?} downgrade is rejected"
    );
}

fn prior_formula(
    spec: &HomebrewFormulaSpec,
    policy: &crate::config::RustBinaryReleaseConfig,
    channel: PackageReleaseChannel,
    next_projection: &ValidatedHomebrewProjection,
) -> Vec<u8> {
    let old_projection =
        validate_homebrew_projection(spec, &jackin_plan_from_policy(policy, channel, "1.2.3"))
            .expect("prior projection");
    let (old_version, old_tag) = match channel {
        PackageReleaseChannel::Preview => {
            ("1.2.3-preview.4+0123456", format!("preview-{SOURCE_SHA}"))
        }
        PackageReleaseChannel::Stable => ("1.2.3", "v1.2.3".to_owned()),
    };
    let old_literals = formula_literals(
        spec,
        &old_projection,
        SOURCE_SHA,
        old_version,
        &old_tag,
        'a',
    );
    let old_bytes = render_homebrew_formula(spec, &old_literals).expect("old formula");
    let parsed_old = parse_generated_formula(spec, next_projection, &old_bytes)
        .expect("old formula parses against next projection");
    assert_eq!(parsed_old.version(), old_version);

    old_bytes
}
