// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::super::{
    PACKAGE_RELEASE_EVIDENCE_PROFILE, PACKAGE_RELEASE_SCHEMA_V2, PackageReleaseAsset,
    PackageReleaseAssetKind, PackageReleaseChannel, PackageReleaseCompanion,
    PackageReleaseEvidence, PackageReleasePayload, PackageReleaseProduct, PackageReleaseReceipt,
    PackageReleaseRepository, PackageReleaseSbomFormat, PackageReleaseSource,
    PackageReleaseVersion, PackageReleaseWorkflow,
};

pub(crate) fn fixture(channel: PackageReleaseChannel) -> PackageReleaseReceipt {
    let base = "1.2.3";
    let commit = "abcdef0123456789abcdef0123456789abcdef01";
    let source_sha7 = (channel == PackageReleaseChannel::Preview).then(|| commit[..7].to_owned());
    let commit_count = (channel == PackageReleaseChannel::Preview).then_some(9);
    let value = preview_value(base, commit_count, source_sha7.as_deref());
    let tag = match channel {
        PackageReleaseChannel::Preview => format!("preview-{commit}"),
        PackageReleaseChannel::Stable => format!("v{base}"),
    };
    PackageReleaseReceipt {
        schema: PACKAGE_RELEASE_SCHEMA_V2.to_owned(),
        channel,
        repository: PackageReleaseRepository {
            slug: "jackin-project/jackin".to_owned(),
            id: 123,
        },
        source: fixture_source(channel, commit),
        workflow: fixture_workflow(channel, commit),
        version: PackageReleaseVersion {
            base: base.to_owned(),
            value,
            commit_count,
            source_sha7,
        },
        tag,
        products: fixture_products(base),
        companions: vec![PackageReleaseCompanion {
            id: "capsule-manifest".to_owned(),
            name: "capsule-manifest.json".to_owned(),
        }],
        assets: fixture_assets(),
        evidence: fixture_evidence(channel),
    }
}

pub(super) fn preview_value(base: &str, count: Option<u64>, sha7: Option<&str>) -> String {
    count.map_or_else(
        || base.to_owned(),
        |count| format!("{base}-preview.{count}+{}", sha7.unwrap_or_default()),
    )
}

pub(super) fn fixture_source(channel: PackageReleaseChannel, commit: &str) -> PackageReleaseSource {
    PackageReleaseSource {
        commit_sha: commit.to_owned(),
        source_ref: "refs/heads/main".to_owned(),
        default_branch: "main".to_owned(),
        event: channel.event(),
        ci_run_id: 100,
        ci_run_attempt: 1,
    }
}

pub(super) fn fixture_workflow(
    channel: PackageReleaseChannel,
    commit: &str,
) -> PackageReleaseWorkflow {
    let path = channel.workflow_path().to_owned();
    PackageReleaseWorkflow {
        path: path.clone(),
        workflow_ref: "refs/heads/main".to_owned(),
        authority_sha: commit.to_owned(),
        run_id: 200,
        run_attempt: 1,
        producer_run_id: 200,
        producer_run_attempt: 1,
        signer_job_id: channel.signer_job_id().to_owned(),
        signer_workflow: path,
    }
}

pub(super) fn fixture_products(base: &str) -> Vec<PackageReleaseProduct> {
    vec![PackageReleaseProduct {
        id: "jackin".to_owned(),
        package: "jackin".to_owned(),
        package_version: base.to_owned(),
        bins: vec!["jackin".to_owned()],
        payloads: vec![PackageReleasePayload {
            format: crate::package_release::PackageReleasePayloadFormat::TarGz,
            target: "aarch64-apple-darwin".to_owned(),
            name: "jackin-aarch64-apple-darwin.tar.gz".to_owned(),
            members: vec!["jackin".to_owned()],
        }],
    }]
}

pub(super) fn fixture_assets() -> Vec<PackageReleaseAsset> {
    let archive = "jackin-aarch64-apple-darwin.tar.gz";
    let mut assets = vec![
        asset(
            archive,
            PackageReleaseAssetKind::Payload,
            Some("jackin"),
            Some("aarch64-apple-darwin"),
            Some(archive),
        ),
        asset(
            &format!("{archive}.sha256"),
            PackageReleaseAssetKind::Sha256,
            Some("jackin"),
            Some("aarch64-apple-darwin"),
            Some(archive),
        ),
        asset(
            &format!("{archive}.bundle"),
            PackageReleaseAssetKind::SignatureBundle,
            Some("jackin"),
            Some("aarch64-apple-darwin"),
            Some(archive),
        ),
        asset(
            &format!("{archive}.sbom.json"),
            PackageReleaseAssetKind::Sbom,
            Some("jackin"),
            Some("aarch64-apple-darwin"),
            Some(archive),
        ),
        asset(
            "capsule-manifest.json",
            PackageReleaseAssetKind::Companion,
            None,
            None,
            Some("capsule-manifest.json"),
        ),
        asset(
            "capsule-manifest.json.bundle",
            PackageReleaseAssetKind::CompanionSignatureBundle,
            None,
            None,
            Some("capsule-manifest.json"),
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

pub(super) fn fixture_evidence(channel: PackageReleaseChannel) -> PackageReleaseEvidence {
    let mut subjects = vec![
        "jackin-aarch64-apple-darwin.tar.gz".to_owned(),
        "capsule-manifest.json".to_owned(),
        "release.json".to_owned(),
    ];
    subjects.sort();
    PackageReleaseEvidence {
        profile: PACKAGE_RELEASE_EVIDENCE_PROFILE.to_owned(),
        sbom_format: PackageReleaseSbomFormat::CycloneDxJson,
        signer_issuer: "https://token.actions.githubusercontent.com".to_owned(),
        signer_workflow_ref: format!(
            "jackin-project/jackin/{}@refs/heads/main",
            channel.workflow_path()
        ),
        attestation_predicate: "https://slsa.dev/provenance/v1".to_owned(),
        attested_subject_names: subjects,
    }
}

pub(super) fn asset(
    name: &str,
    kind: PackageReleaseAssetKind,
    product_id: Option<&str>,
    target: Option<&str>,
    subject: Option<&str>,
) -> PackageReleaseAsset {
    PackageReleaseAsset {
        name: name.to_owned(),
        kind,
        product_id: product_id.map(str::to_owned),
        target: target.map(str::to_owned),
        subject_name: subject.map(str::to_owned),
        size: 1,
        sha256: "a".repeat(64),
    }
}

#[path = "package_release_test_support_inventory.rs"]
mod inventory;
pub(crate) use inventory::*;
