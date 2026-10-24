use std::collections::BTreeMap;

use super::*;
use crate::package_release::tests::support;
use crate::package_release_plan::{PACKAGE_RELEASE_PLAN_SCHEMA_V1, PackageReleasePlanProduct};

const SOURCE_SHA: &str = "0123456789abcdef0123456789abcdef01234567";
const BASE_SHA: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const NEXT_SHA: &str = "abcdef0123456789abcdef0123456789abcdef01";

fn assert_formula_blocks_balance(text: &str) {
    let mut depth = 0usize;
    for (number, line) in text.lines().enumerate() {
        let line = line.trim();
        if line == "end" {
            depth = depth
                .checked_sub(1)
                .unwrap_or_else(|| panic!("unmatched end at line {}: {line}\n{text}", number + 1));
        } else if line.starts_with("class ")
            || line.starts_with("def ")
            || line.starts_with("if ")
            || line.ends_with(" do")
        {
            depth += 1;
        }
    }
    assert_eq!(depth, 0, "all fixed formula blocks close");
}

fn spec() -> HomebrewFormulaSpec {
    HomebrewFormulaSpec {
        schema: HOMEBREW_FORMULA_SPEC_SCHEMA_V1.to_owned(),
        template: HOMEBREW_FORMULA_TEMPLATE_V1.to_owned(),
        description: "Jackin #{system} #@instance #$global app \"quoted\" safely".to_owned(),
        homepage: "https://github.com/jackin-project/jackin".to_owned(),
        license: "MIT".to_owned(),
        channels: vec![
            HomebrewFormulaChannelSpec {
                channel: PackageReleaseChannel::Preview,
                formula: "jackin-preview".to_owned(),
                formula_class: "JackinPreview".to_owned(),
                destination: "Formula/jackin-preview.rb".to_owned(),
                conflicts: vec!["jackin".to_owned()],
            },
            HomebrewFormulaChannelSpec {
                channel: PackageReleaseChannel::Stable,
                formula: "jackin".to_owned(),
                formula_class: "Jackin".to_owned(),
                destination: "Formula/jackin.rb".to_owned(),
                conflicts: vec!["jackin-preview".to_owned()],
            },
        ],
        host: HomebrewHostProductSpec {
            product: "jackin".to_owned(),
            targets: vec![
                "aarch64-apple-darwin".to_owned(),
                "aarch64-unknown-linux-gnu".to_owned(),
                "x86_64-apple-darwin".to_owned(),
                "x86_64-unknown-linux-gnu".to_owned(),
            ],
            bins: vec![
                "jackin".to_owned(),
                "jackin-role".to_owned(),
                "jackin-usage-broker".to_owned(),
            ],
        },
        auxiliaries: vec![HomebrewAuxiliaryProductSpec {
            product: "jackin-capsule".to_owned(),
            targets: vec![
                "aarch64-unknown-linux-gnu".to_owned(),
                "x86_64-unknown-linux-gnu".to_owned(),
            ],
            files: vec![
                HomebrewAuxiliaryFileSpec {
                    target: "aarch64-unknown-linux-gnu".to_owned(),
                    member: "jackin-capsule".to_owned(),
                    destination: "jackin-capsule/linux-arm64/jackin-capsule".to_owned(),
                    mode: HomebrewInstalledMode::Executable,
                    test: HomebrewAuxiliaryTest::LinuxElf,
                },
                HomebrewAuxiliaryFileSpec {
                    target: "x86_64-unknown-linux-gnu".to_owned(),
                    member: "jackin-capsule".to_owned(),
                    destination: "jackin-capsule/linux-x64/jackin-capsule".to_owned(),
                    mode: HomebrewInstalledMode::Executable,
                    test: HomebrewAuxiliaryTest::LinuxElf,
                },
            ],
        }],
        stable_sentinel: HomebrewStableSentinel {
            date: "2026-10-24".to_owned(),
            reason: "Stable release is not active".to_owned(),
        },
    }
}

fn plan(channel: PackageReleaseChannel) -> PackageReleasePlan {
    serde_json::from_value(serde_json::json!({
        "schema": "velnor.package-release-plan.v1",
        "channel": channel,
        "repository": "jackin-project/jackin",
        "default_branch": "main",
        "base_version": "1.2.3",
        "products": [
            {
                "id": "jackin",
                "package": "jackin",
                "bins": ["jackin", "jackin-role", "jackin-usage-broker"],
                "targets": ["aarch64-apple-darwin", "aarch64-unknown-linux-gnu", "x86_64-apple-darwin", "x86_64-unknown-linux-gnu"],
                "format": "tar-gz"
            },
            {
                "id": "jackin-capsule",
                "package": "jackin-capsule",
                "bins": ["jackin-capsule"],
                "targets": ["aarch64-unknown-linux-gnu", "x86_64-unknown-linux-gnu"],
                "format": "tar-gz"
            }
        ],
        "companions": []
    }))
    .expect("valid Jackin release plan")
}

fn literals(
    spec: &HomebrewFormulaSpec,
    projection: &ValidatedHomebrewProjection,
) -> HomebrewFormulaReleaseLiterals {
    formula_literals(
        spec,
        projection,
        SOURCE_SHA,
        "1.2.3-preview.4+0123456",
        &format!("preview-{SOURCE_SHA}"),
        'a',
    )
}

fn formula_literals(
    spec: &HomebrewFormulaSpec,
    projection: &ValidatedHomebrewProjection,
    source_sha: &str,
    version: &str,
    tag: &str,
    digest_char: char,
) -> HomebrewFormulaReleaseLiterals {
    let hashes = projection
        .selected_payloads()
        .iter()
        .map(|payload| {
            (
                payload.asset_name().to_owned(),
                digest_char.to_string().repeat(64),
            )
        })
        .collect::<BTreeMap<_, _>>();
    HomebrewFormulaReleaseLiterals::new(
        spec,
        projection,
        source_sha.to_owned(),
        version.to_owned(),
        tag.to_owned(),
        hashes,
    )
    .expect("valid release literals")
}

fn jackin_plan_from_policy(
    policy: &crate::config::RustBinaryReleaseConfig,
    channel: PackageReleaseChannel,
    base_version: &str,
) -> PackageReleasePlan {
    let mut products = policy
        .products
        .iter()
        .map(|product| PackageReleasePlanProduct {
            id: product.id.clone(),
            package: product.package.clone(),
            bins: product.bins.clone(),
            targets: product.targets.clone(),
            format: crate::package_release::PackageReleasePayloadFormat::TarGz,
        })
        .collect::<Vec<_>>();
    products.sort_by(|left, right| left.id.cmp(&right.id));
    for product in &mut products {
        product.bins.sort();
        product.targets.sort();
    }
    let mut companions = policy.companions.clone();
    companions.sort_by(|left, right| left.id.cmp(&right.id));
    PackageReleasePlan {
        schema: PACKAGE_RELEASE_PLAN_SCHEMA_V1.to_owned(),
        channel,
        repository: "jackin-project/jackin".to_owned(),
        default_branch: "main".to_owned(),
        base_version: base_version.to_owned(),
        products,
        companions,
    }
}

fn next_verified_release(
    channel: PackageReleaseChannel,
    base_version: &str,
) -> crate::homebrew_formula::VerifiedPackageRelease {
    let mut receipt = support::jackin_receipt(&support::jackin_preview());
    receipt.source.commit_sha = NEXT_SHA.to_owned();
    receipt.workflow.authority_sha = NEXT_SHA.to_owned();
    receipt.version.base = base_version.to_owned();
    receipt.channel = channel;
    match channel {
        PackageReleaseChannel::Preview => {
            receipt.version.commit_count = Some(10);
            receipt.version.source_sha7 = Some(NEXT_SHA[..7].to_owned());
            receipt.version.value = format!("{base_version}-preview.10+{}", &NEXT_SHA[..7]);
            receipt.tag = format!("preview-{NEXT_SHA}");
        }
        PackageReleaseChannel::Stable => {
            receipt.source.event = crate::package_release::PackageReleaseEvent::WorkflowDispatch;
            receipt.version.commit_count = None;
            receipt.version.source_sha7 = None;
            receipt.version.value = base_version.to_owned();
            receipt.tag = format!("v{base_version}");
            receipt.workflow.path = PackageReleaseChannel::Stable.workflow_path().to_owned();
            receipt.workflow.signer_job_id =
                PackageReleaseChannel::Stable.signer_job_id().to_owned();
            receipt.workflow.signer_workflow =
                PackageReleaseChannel::Stable.workflow_path().to_owned();
            receipt.evidence.signer_workflow_ref = format!(
                "{}/{}@{}",
                receipt.repository.slug, receipt.workflow.path, receipt.workflow.workflow_ref
            );
        }
    }
    for product in &mut receipt.products {
        product.package_version = base_version.to_owned();
    }
    receipt.validate().expect("valid next release fixture");
    crate::homebrew_formula::VerifiedPackageRelease {
        receipt_digest: receipt.canonical_digest().expect("receipt digest"),
        receipt,
    }
}

fn receipt_asset_hashes(
    projection: &ValidatedHomebrewProjection,
    release: &crate::homebrew_formula::VerifiedPackageRelease,
) -> BTreeMap<String, String> {
    projection
        .selected_payloads()
        .iter()
        .map(|payload| {
            let asset = release
                .receipt
                .assets
                .iter()
                .find(|asset| asset.name == payload.asset_name())
                .expect("selected payload in receipt");
            (payload.asset_name().to_owned(), asset.sha256.clone())
        })
        .collect()
}

#[path = "homebrew_spec/history_tests.rs"]
mod history;
#[path = "homebrew_spec/installation_tests.rs"]
mod installation;
#[path = "homebrew_spec/literals_tests.rs"]
mod literals;
#[path = "homebrew_spec/projection_tests.rs"]
mod projection;
