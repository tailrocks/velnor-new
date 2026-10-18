//! Shared one-payload tap fixture for writer and merge protocol tests.

use velnor_actions_contract::{
    PACKAGE_RELEASE_PLAN_SCHEMA_V1, PackageReleasePlan, PackageReleasePlanProduct,
    VerifiedPreviewRelease,
    config::{
        CheckExecutor, CheckPlatform, CheckRunner, ExactHeadReviewCheck, HomebrewTapConfig,
        HomebrewTapSource, MiseTaskSource, ReviewAppIdentity, TapMergePolicy, TapUpdateEnvironment,
        TapUpdaterBinding,
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
    pub(crate) plan: PackageReleasePlan,
    pub(crate) release: VerifiedPreviewRelease,
    pub(crate) base_formula: Vec<u8>,
    pub(crate) proposed_formula: Vec<u8>,
}

pub(crate) fn fixture() -> TapFixture {
    let policy = policy();
    let plan = plan();
    let release = release();
    let base_formula = formula();
    let proposed_formula = canonical_formula(&base_formula, &release);
    assert_eq!(
        velnor_actions_contract::validate_homebrew_formula_candidate(
            &velnor_actions_contract::HomebrewFormulaCandidate {
                source_repository: &plan.repository,
                plan: &plan,
                tap_repository: &policy.repository,
                formula_path: &policy.formula,
                captured_base_sha: &"b".repeat(40),
                base_bytes: &base_formula,
                proposed_bytes: &proposed_formula,
                release: &release,
            }
        )
        .expect("fixture formula validates"),
        proposed_formula
    );
    TapFixture {
        policy,
        plan,
        release,
        base_formula,
        proposed_formula,
    }
}

pub(crate) fn policy() -> HomebrewTapConfig {
    HomebrewTapConfig {
        enabled: true,
        source: HomebrewTapSource::RustBinaryReleasePreview,
        repository: "owner/homebrew-tap".to_owned(),
        formula: "Formula/cli-preview.rb".to_owned(),
        updater: TapUpdaterBinding {
            task: "package:update-preview".to_owned(),
            source: MiseTaskSource {
                mise_config: "mise.toml".to_owned(),
                working_directory: ".".to_owned(),
            },
            runner: CheckRunner {
                label: "ubuntu-26.04".to_owned(),
                platform: CheckPlatform::LinuxX64,
                executor: CheckExecutor::Hosted,
                container: None,
            },
            inputs: vec![
                "Formula/cli-preview.rb".to_owned(),
                "mise.toml".to_owned(),
                "scripts/update.sh".to_owned(),
            ],
            tools: vec!["jq".to_owned()],
            timeout_minutes: 5,
        },
        environment: TapUpdateEnvironment::TapUpdate,
        app_id_variable: "TAP_APP_ID".to_owned(),
        app_private_key_secret: "TAP_APP_PRIVATE_KEY".to_owned(),
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

pub(crate) fn release() -> VerifiedPreviewRelease {
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
            event: PackageReleaseEvent::Push,
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
        tag: format!("v{version}"),
        products: vec![release_product(archive)],
        companions: Vec::new(),
        assets,
        evidence: release_evidence(archive, workflow_path),
    };
    receipt.validate().expect("fixture receipt validates");
    let receipt_digest = receipt.canonical_digest().expect("receipt digest");
    VerifiedPreviewRelease {
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

fn formula() -> Vec<u8> {
    format!(
        "# source-sha: {}\nclass CliPreview < Formula\n  version \"{}\"\n      url \"https://github.com/owner/source/releases/download/{}/cli-aarch64-apple-darwin.tar.gz\"\n      sha256 \"{}\"\n  def install\n    bin.install \"cli\"\n  end\nend\n",
        "bbbbbbb000000000000000000000000000000000",
        "1.2.3-preview.8+bbbbbbb",
        "v1.2.3-preview.8+bbbbbbb",
        "a".repeat(64),
    )
    .into_bytes()
}

fn canonical_formula(base: &[u8], release: &VerifiedPreviewRelease) -> Vec<u8> {
    let text = std::str::from_utf8(base).expect("formula UTF-8");
    let archive = &release.receipt.products[0].payloads[0].name;
    text.replace(
        "bbbbbbb000000000000000000000000000000000",
        &release.receipt.source.commit_sha,
    )
    .replace("1.2.3-preview.8+bbbbbbb", &release.receipt.version.value)
    .replace("v1.2.3-preview.8+bbbbbbb", &release.receipt.tag)
    .replace("cli-aarch64-apple-darwin.tar.gz", archive)
    .into_bytes()
}
