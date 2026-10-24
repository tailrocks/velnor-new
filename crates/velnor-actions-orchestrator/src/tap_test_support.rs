//! Shared one-payload tap fixture for writer and merge protocol tests.

use velnor_actions_contract::{
    PACKAGE_RELEASE_PLAN_SCHEMA_V1, PackageReleasePlan, PackageReleasePlanProduct,
    VerifiedPackageRelease,
    config::{
        ExactHeadReviewCheck, HomebrewTapConfig, HomebrewTapDestination, HomebrewTapDestinations,
        ReviewAppIdentity, TapMergePolicy, TapUpdateEnvironment,
    },
    homebrew_spec::{
        HOMEBREW_FORMULA_SPEC_SCHEMA_V1, HOMEBREW_FORMULA_TEMPLATE_V1, HomebrewFormulaChannelSpec,
        HomebrewFormulaReleaseLiterals, HomebrewFormulaSpec, HomebrewHostProductSpec,
        HomebrewStableSentinel, render_homebrew_formula, validate_homebrew_projection,
    },
    package_release::{
        PACKAGE_RELEASE_EVIDENCE_PROFILE, PACKAGE_RELEASE_SCHEMA_V2, PackageReleaseAsset,
        PackageReleaseAssetKind, PackageReleaseChannel, PackageReleaseEvent,
        PackageReleaseEvidence, PackageReleasePayload, PackageReleasePayloadFormat,
        PackageReleaseProduct, PackageReleaseReceipt, PackageReleaseRepository,
        PackageReleaseSbomFormat, PackageReleaseSource, PackageReleaseVersion,
        PackageReleaseWorkflow,
    },
};

pub(crate) struct TapFixture {
    pub(crate) policy: HomebrewTapConfig,
    pub(crate) spec: HomebrewFormulaSpec,
    pub(crate) plan: PackageReleasePlan,
    pub(crate) release: VerifiedPackageRelease,
    pub(crate) base_formula: Vec<u8>,
    pub(crate) proposed_formula: Vec<u8>,
    pub(crate) formula_path: String,
}
pub(crate) fn fixture() -> TapFixture {
    let policy = policy();
    let plan = plan();
    let release = release();
    let spec = spec();
    let formula_path = policy
        .destinations
        .preview
        .as_ref()
        .expect("preview")
        .formula
        .clone();
    let projection = validate_homebrew_projection(&spec, &plan).expect("projection");
    let release_hashes = release
        .receipt
        .assets
        .iter()
        .filter(|asset| asset.kind == PackageReleaseAssetKind::Payload)
        .map(|asset| (asset.name.clone(), asset.sha256.clone()))
        .collect();
    let release_literals = HomebrewFormulaReleaseLiterals::new(
        &spec,
        &projection,
        release.receipt.source.commit_sha.clone(),
        release.receipt.version.value.clone(),
        release.receipt.tag.clone(),
        release_hashes,
    )
    .expect("release literals");
    let proposed_formula = render_homebrew_formula(&spec, &release_literals).expect("formula");
    let old_source = "bbbbbbb000000000000000000000000000000000";
    let old_literals = HomebrewFormulaReleaseLiterals::new(
        &spec,
        &projection,
        old_source.to_owned(),
        "1.2.3-preview.8+bbbbbbb".to_owned(),
        format!("preview-{old_source}"),
        projection
            .selected_payloads()
            .iter()
            .map(|payload| (payload.asset_name().to_owned(), "a".repeat(64)))
            .collect(),
    )
    .expect("old formula literals");
    let base_formula = render_homebrew_formula(&spec, &old_literals).expect("old formula");
    TapFixture {
        policy,
        spec,
        plan,
        release,
        base_formula,
        proposed_formula,
        formula_path,
    }
}

pub(crate) fn spec() -> HomebrewFormulaSpec {
    HomebrewFormulaSpec {
        schema: HOMEBREW_FORMULA_SPEC_SCHEMA_V1.to_owned(),
        template: HOMEBREW_FORMULA_TEMPLATE_V1.to_owned(),
        description: "Example CLI".to_owned(),
        homepage: "https://example.com/project".to_owned(),
        license: "MIT".to_owned(),
        channels: vec![
            HomebrewFormulaChannelSpec {
                channel: PackageReleaseChannel::Preview,
                formula: "cli-preview".to_owned(),
                formula_class: "CliPreview".to_owned(),
                destination: "Formula/cli-preview.rb".to_owned(),
                conflicts: vec!["cli".to_owned()],
            },
            HomebrewFormulaChannelSpec {
                channel: PackageReleaseChannel::Stable,
                formula: "cli".to_owned(),
                formula_class: "Cli".to_owned(),
                destination: "Formula/cli.rb".to_owned(),
                conflicts: vec!["cli-preview".to_owned()],
            },
        ],
        host: HomebrewHostProductSpec {
            product: "cli".to_owned(),
            targets: vec!["aarch64-apple-darwin".to_owned()],
            bins: vec!["cli".to_owned()],
        },
        auxiliaries: Vec::new(),
        stable_sentinel: HomebrewStableSentinel {
            date: "2026-10-24".to_owned(),
            reason: "Stable release is not active".to_owned(),
        },
    }
}

pub(crate) fn policy() -> HomebrewTapConfig {
    HomebrewTapConfig {
        enabled: true,
        repository: "owner/homebrew-tap".to_owned(),
        updater: None,
        destinations: HomebrewTapDestinations {
            preview: Some(HomebrewTapDestination {
                formula: "Formula/cli-preview.rb".to_owned(),
            }),
            stable: Some(HomebrewTapDestination {
                formula: "Formula/cli.rb".to_owned(),
            }),
        },
        environment: TapUpdateEnvironment::TapUpdate,
        app_id_variable: "TAP_APP_ID".to_owned(),
        app_private_key_secret: "TAP_APP_PRIVATE_KEY".to_owned(),
        protection_observer: velnor_actions_contract::config::HomebrewTapProtectionObserver {
            app_id_variable: "TAP_PROTECTION_APP_ID".to_owned(),
            app_private_key_secret: "TAP_PROTECTION_APP_PRIVATE_KEY".to_owned(),
        },
        merge: TapMergePolicy::SquashAfterGates,
        review_completion: ExactHeadReviewCheck {
            check_name: "Review completion".to_owned(),
            app_id_variable: "REVIEW_APP_ID".to_owned(),
            expected_reviewers: vec![ReviewAppIdentity {
                app_id_variable: "REVIEWER_APP_ID".to_owned(),
            }],
        },
    }
}

pub(crate) fn plan() -> PackageReleasePlan {
    PackageReleasePlan {
        schema: PACKAGE_RELEASE_PLAN_SCHEMA_V1.to_owned(),
        channel: PackageReleaseChannel::Preview,
        repository: "owner/source".to_owned(),
        default_branch: "main".to_owned(),
        base_version: "1.2.3".to_owned(),
        products: vec![PackageReleasePlanProduct {
            id: "cli".to_owned(),
            package: "cli".to_owned(),
            bins: vec!["cli".to_owned()],
            targets: vec!["aarch64-apple-darwin".to_owned()],
            format: PackageReleasePayloadFormat::TarGz,
        }],
        companions: Vec::new(),
    }
}

pub(crate) fn release() -> VerifiedPackageRelease {
    let source_sha = "abcdef0123456789abcdef0123456789abcdef01";
    let archive = "cli-aarch64-apple-darwin.tar.gz";
    let version = "1.2.3-preview.9+abcdef0";
    let assets = release_assets(archive);
    let workflow_path = PackageReleaseChannel::Preview.workflow_path();
    let receipt = PackageReleaseReceipt {
        schema: PACKAGE_RELEASE_SCHEMA_V2.to_owned(),
        channel: PackageReleaseChannel::Preview,
        repository: PackageReleaseRepository {
            slug: "owner/source".to_owned(),
            id: 41,
        },
        source: PackageReleaseSource {
            commit_sha: source_sha.to_owned(),
            source_ref: "refs/heads/main".to_owned(),
            default_branch: "main".to_owned(),
            event: PackageReleaseEvent::WorkflowRun,
            ci_run_id: 7,
            ci_run_attempt: 1,
        },
        workflow: PackageReleaseWorkflow {
            path: workflow_path.to_owned(),
            workflow_ref: "refs/heads/main".to_owned(),
            authority_sha: source_sha.to_owned(),
            run_id: 8,
            run_attempt: 1,
            producer_run_id: 8,
            producer_run_attempt: 1,
            signer_job_id: "preview-signer".to_owned(),
            signer_workflow: workflow_path.to_owned(),
        },
        version: PackageReleaseVersion {
            base: "1.2.3".to_owned(),
            value: version.to_owned(),
            commit_count: Some(9),
            source_sha7: Some(source_sha[..7].to_owned()),
        },
        tag: format!("preview-{source_sha}"),
        products: vec![release_product(archive)],
        companions: Vec::new(),
        assets,
        evidence: release_evidence(archive, workflow_path),
    };
    receipt.validate().expect("fixture receipt validates");
    let receipt_digest = receipt.canonical_digest().expect("receipt digest");
    VerifiedPackageRelease {
        receipt_digest,
        receipt,
    }
}

fn release_assets(archive: &str) -> Vec<PackageReleaseAsset> {
    let mut assets = vec![
        asset(
            archive,
            PackageReleaseAssetKind::Payload,
            Some("cli"),
            Some("aarch64-apple-darwin"),
            Some(archive),
        ),
        asset(
            &format!("{archive}.sha256"),
            PackageReleaseAssetKind::Sha256,
            Some("cli"),
            Some("aarch64-apple-darwin"),
            Some(archive),
        ),
        asset(
            &format!("{archive}.bundle"),
            PackageReleaseAssetKind::SignatureBundle,
            Some("cli"),
            Some("aarch64-apple-darwin"),
            Some(archive),
        ),
        asset(
            &format!("{archive}.sbom.json"),
            PackageReleaseAssetKind::Sbom,
            Some("cli"),
            Some("aarch64-apple-darwin"),
            Some(archive),
        ),
        asset(
            "SHA256SUMS",
            PackageReleaseAssetKind::ChecksumList,
            None,
            None,
            None,
        ),
    ];
    assets.sort_by(|left, right| left.name.cmp(&right.name));
    assets
}

fn release_product(archive: &str) -> PackageReleaseProduct {
    PackageReleaseProduct {
        id: "cli".to_owned(),
        package: "cli".to_owned(),
        package_version: "1.2.3".to_owned(),
        bins: vec!["cli".to_owned()],
        payloads: vec![PackageReleasePayload {
            format: PackageReleasePayloadFormat::TarGz,
            target: "aarch64-apple-darwin".to_owned(),
            name: archive.to_owned(),
            members: vec!["cli".to_owned()],
        }],
    }
}

fn release_evidence(archive: &str, workflow_path: &str) -> PackageReleaseEvidence {
    PackageReleaseEvidence {
        profile: PACKAGE_RELEASE_EVIDENCE_PROFILE.to_owned(),
        sbom_format: PackageReleaseSbomFormat::CycloneDxJson,
        signer_issuer: "https://token.actions.githubusercontent.com".to_owned(),
        signer_workflow_ref: format!("owner/source/{workflow_path}@refs/heads/main"),
        attestation_predicate: "https://slsa.dev/provenance/v1".to_owned(),
        attested_subject_names: vec![archive.to_owned(), "release.json".to_owned()],
    }
}

fn asset(
    name: &str,
    kind: PackageReleaseAssetKind,
    product_id: Option<&str>,
    target: Option<&str>,
    subject_name: Option<&str>,
) -> PackageReleaseAsset {
    PackageReleaseAsset {
        name: name.to_owned(),
        kind,
        product_id: product_id.map(str::to_owned),
        target: target.map(str::to_owned),
        subject_name: subject_name.map(str::to_owned),
        size: 1,
        sha256: "a".repeat(64),
    }
}
