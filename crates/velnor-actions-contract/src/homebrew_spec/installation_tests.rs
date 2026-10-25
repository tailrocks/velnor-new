use super::super::tests::*;

#[test]
fn spec_rejects_ambiguous_cpu_targets_effective_path_collisions_and_unicode_metadata() {
    let baseline = spec();
    let mut ambiguous = baseline.clone();
    ambiguous.auxiliaries[0]
        .targets
        .insert(0, "aarch64-apple-darwin".to_owned());
    ambiguous.auxiliaries[0].files.insert(
        0,
        HomebrewAuxiliaryFileSpec {
            target: "aarch64-apple-darwin".to_owned(),
            member: "jackin-capsule".to_owned(),
            destination: "jackin-capsule/macos-arm64/jackin-capsule".to_owned(),
            mode: HomebrewInstalledMode::Executable,
            test: HomebrewAuxiliaryTest::Executable,
        },
    );
    assert!(ambiguous.validate().is_err());

    for destination in [
        "jackin-capsule/linux-arm64/jackin-capsule",
        "jackin-capsule",
    ] {
        let mut colliding = baseline.clone();
        colliding.auxiliaries.push(HomebrewAuxiliaryProductSpec {
            product: "jackin-sidecar".to_owned(),
            targets: vec!["aarch64-unknown-linux-gnu".to_owned()],
            files: vec![HomebrewAuxiliaryFileSpec {
                target: "aarch64-unknown-linux-gnu".to_owned(),
                member: "jackin-sidecar".to_owned(),
                destination: destination.to_owned(),
                mode: HomebrewInstalledMode::Executable,
                test: HomebrewAuxiliaryTest::LinuxElf,
            }],
        });
        assert!(colliding.validate().is_err(), "destination {destination}");
    }

    let mut unicode = baseline;
    unicode.description = "Jackin command line tools – preview".to_owned();
    assert!(unicode.validate().is_err());
    unicode.description = "Jackin command line tools".to_owned();
    unicode.stable_sentinel.reason = "Pré-release".to_owned();
    assert!(unicode.validate().is_err());
    unicode.stable_sentinel.reason = "Stable release is not active".to_owned();
    unicode.homepage = "https://example.com/café".to_owned();
    assert!(unicode.validate().is_err());
}

#[test]
fn auxiliary_installation_enforces_data_and_executable_modes() {
    let mut data_spec = spec();
    data_spec.auxiliaries[0].files[0].mode = HomebrewInstalledMode::Data;
    data_spec.auxiliaries[0].files[0].test = HomebrewAuxiliaryTest::None;
    let projection =
        validate_homebrew_projection(&data_spec, &plan(PackageReleaseChannel::Preview))
            .expect("data projection");
    let formula = String::from_utf8(
        render_homebrew_formula(&data_spec, &literals(&data_spec, &projection)).expect("render"),
    )
    .expect("UTF-8");
    assert!(
        formula.contains("(libexec/\"jackin-capsule/linux-arm64/jackin-capsule\").chmod(0644)")
    );
    assert!(formula.contains("(libexec/\"jackin-capsule/linux-x64/jackin-capsule\").chmod(0755)"));
}

#[test]
fn stable_sentinel_keeps_same_literal_slots_and_activation_is_literal_only() {
    let spec = spec();
    let mut colliding_plan = plan(PackageReleaseChannel::Stable);
    colliding_plan.base_version = "0.0.0".to_owned();
    let colliding_projection =
        validate_homebrew_projection(&spec, &colliding_plan).expect("stable projection");
    assert!(HomebrewFormulaReleaseLiterals::stable_sentinel(&spec, &colliding_projection).is_err());
    let stable_plan = plan(PackageReleaseChannel::Stable);
    let stable_projection =
        validate_homebrew_projection(&spec, &stable_plan).expect("stable projection");
    let sentinel = HomebrewFormulaReleaseLiterals::stable_sentinel(&spec, &stable_projection)
        .expect("sentinel");
    let sentinel_bytes = render_homebrew_formula(&spec, &sentinel).expect("disabled formula");
    let sentinel_text = std::str::from_utf8(&sentinel_bytes).expect("UTF-8 formula");
    assert!(sentinel_text.contains("if version.to_s == \"0.0.0\"\n    disable! date:"));
    assert!(!sentinel_text.contains(
        "disable! date: \\\"2026-10-24\\\", because: \\\"Stable release is not active\\\", version:"
    ));
    assert_eq!(sentinel_text.matches("url ").count(), 6);
    assert_eq!(sentinel_text.matches("sha256 ").count(), 6);
    assert_eq!(sentinel_text.matches("bin.install ").count(), 3);
    assert!(sentinel_text.contains("def install"));
    assert!(sentinel_text.contains("test do"));
    assert_eq!(
        parse_generated_formula(&spec, &stable_projection, &sentinel_bytes)
            .expect("recognize sentinel"),
        sentinel
    );

    let (literals, actual) = stable_activation_literals(&spec, &stable_projection);
    let activated = render_homebrew_formula(&spec, &literals).expect("activated formula");
    assert_eq!(
        normalized(sentinel_text),
        normalized(std::str::from_utf8(&activated).expect("activated UTF-8"))
    );

    let plan = jackin_plan_from_policy(
        &support::jackin_preview(),
        PackageReleaseChannel::Stable,
        "1.2.3",
    );
    let digest = actual.canonical_digest().expect("receipt digest");
    let verified = crate::homebrew_formula::VerifiedPackageRelease {
        receipt_digest: digest,
        receipt: actual,
    };
    let candidate = crate::homebrew_formula::HomebrewFormulaCandidate {
        source_repository: "jackin-project/jackin",
        plan: &plan,
        tap_repository: "jackin-project/homebrew-tap",
        formula_path: "Formula/jackin.rb",
        captured_base_sha: BASE_SHA,
        base_bytes: &sentinel_bytes,
        proposed_bytes: &activated,
        release: &verified,
    };
    assert_eq!(
        crate::homebrew_formula::validate_homebrew_formula_candidate(&candidate)
            .expect("literal-only activation"),
        activated
    );

    let retry = crate::homebrew_formula::HomebrewFormulaCandidate {
        base_bytes: &activated,
        proposed_bytes: &activated,
        ..candidate
    };
    assert_eq!(
        crate::homebrew_formula::validate_homebrew_formula_candidate(&retry)
            .expect("stable no-op retry"),
        activated
    );
}

fn normalized(text: &str) -> Vec<String> {
    text.lines()
        .map(|line| {
            if line.starts_with("# source-sha: ") {
                "# source-sha: <literal>".to_owned()
            } else if line.starts_with("  version ") {
                "  version <literal>".to_owned()
            } else if line.trim_start().starts_with("url ") {
                "url <literal>".to_owned()
            } else if line.trim_start().starts_with("sha256 ") {
                "sha256 <literal>".to_owned()
            } else {
                line.to_owned()
            }
        })
        .collect::<Vec<_>>()
}

fn stable_activation_literals(
    spec: &HomebrewFormulaSpec,
    stable_projection: &ValidatedHomebrewProjection,
) -> (
    HomebrewFormulaReleaseLiterals,
    crate::package_release::PackageReleaseReceipt,
) {
    let mut actual = support::jackin_receipt(&support::jackin_preview());
    actual.channel = PackageReleaseChannel::Stable;
    actual.source.commit_sha = "abcdef0123456789abcdef0123456789abcdef01".to_owned();
    actual.workflow.authority_sha = actual.source.commit_sha.clone();
    actual.version.value = actual.version.base.clone();
    actual.version.commit_count = None;
    actual.version.source_sha7 = None;
    actual.tag = format!("v{}", actual.version.base);
    actual.source.event = crate::package_release::PackageReleaseEvent::WorkflowDispatch;
    actual.workflow.path = PackageReleaseChannel::Stable.workflow_path().to_owned();
    actual.workflow.signer_job_id = PackageReleaseChannel::Stable.signer_job_id().to_owned();
    actual.workflow.signer_workflow = PackageReleaseChannel::Stable.workflow_path().to_owned();
    actual.evidence.signer_workflow_ref = format!(
        "{}/{}@{}",
        actual.repository.slug, actual.workflow.path, actual.workflow.workflow_ref
    );
    actual.validate().expect("valid fixture stable release");
    let hashes = stable_projection
        .selected_payloads()
        .iter()
        .map(|payload| {
            let asset = actual
                .assets
                .iter()
                .find(|asset| asset.name == payload.asset_name())
                .expect("selected payload digest");
            (payload.asset_name().to_owned(), asset.sha256.clone())
        })
        .collect();
    let literals = HomebrewFormulaReleaseLiterals::new(
        spec,
        stable_projection,
        actual.source.commit_sha.clone(),
        actual.version.value.clone(),
        actual.tag.clone(),
        hashes,
    )
    .expect("stable release literals");
    (literals, actual)
}
