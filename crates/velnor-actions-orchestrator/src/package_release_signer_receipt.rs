use velnor_actions_contract::{
    PackageReleasePlan,
    package_release::{
        PACKAGE_RELEASE_EVIDENCE_PROFILE, PackageReleaseAssetKind, PackageReleaseChannel,
        PackageReleaseCompanion, PackageReleaseEvidence, PackageReleasePayload,
        PackageReleasePayloadFormat, PackageReleaseProduct, PackageReleaseReceipt,
        PackageReleaseRepository, PackageReleaseSbomFormat, PackageReleaseSource,
        PackageReleaseVersion, PackageReleaseWorkflow,
    },
};

use super::{PackageReleaseSigningIdentity, VerifiedPackageArtifact};
use crate::{OrchestratorError, internal::internal};

pub(super) fn make_receipt(
    plan: &PackageReleasePlan,
    identity: &PackageReleaseSigningIdentity,
    version: &str,
    tag: &str,
    commit_count: Option<u64>,
    source_sha7: Option<String>,
    artifacts: Vec<VerifiedPackageArtifact>,
) -> PackageReleaseReceipt {
    let subjects = attested_subjects(&artifacts);
    PackageReleaseReceipt {
        schema: velnor_actions_contract::package_release::PACKAGE_RELEASE_SCHEMA_V2.to_owned(),
        channel: plan.channel,
        repository: PackageReleaseRepository {
            slug: identity.repository.clone(),
            id: identity.repository_id,
        },
        source: PackageReleaseSource {
            commit_sha: identity.source_sha.clone(),
            source_ref: identity.source_ref.clone(),
            default_branch: identity.default_branch.clone(),
            event: identity.event,
            ci_run_id: identity.ci_run_id,
            ci_run_attempt: identity.ci_run_attempt,
        },
        workflow: PackageReleaseWorkflow {
            path: identity.workflow_path.clone(),
            workflow_ref: identity.source_ref.clone(),
            authority_sha: identity.workflow_sha.clone(),
            run_id: identity.run_id,
            run_attempt: identity.run_attempt,
            producer_run_id: identity.run_id,
            producer_run_attempt: identity.run_attempt,
            signer_job_id: plan.channel.signer_job_id().to_owned(),
            signer_workflow: identity.workflow_path.clone(),
        },
        version: PackageReleaseVersion {
            base: plan.base_version.clone(),
            value: version.to_owned(),
            commit_count,
            source_sha7,
        },
        tag: tag.to_owned(),
        products: release_products(plan),
        companions: release_companions(plan),
        assets: artifacts
            .into_iter()
            .map(|artifact| artifact.release_asset)
            .collect(),
        evidence: PackageReleaseEvidence {
            profile: PACKAGE_RELEASE_EVIDENCE_PROFILE.to_owned(),
            sbom_format: PackageReleaseSbomFormat::CycloneDxJson,
            signer_issuer: "https://token.actions.githubusercontent.com".to_owned(),
            signer_workflow_ref: format!(
                "{}/{}@{}",
                identity.repository, identity.workflow_path, identity.source_ref
            ),
            attestation_predicate: "https://slsa.dev/provenance/v1".to_owned(),
            attested_subject_names: subjects,
        },
    }
}

fn release_products(plan: &PackageReleasePlan) -> Vec<PackageReleaseProduct> {
    let mut products = plan
        .products
        .iter()
        .map(|product| {
            let mut targets = product.targets.clone();
            targets.sort();
            PackageReleaseProduct {
                id: product.id.clone(),
                package: product.package.clone(),
                package_version: plan.base_version.clone(),
                bins: sorted(product.bins.clone()),
                payloads: targets
                    .into_iter()
                    .map(|target| PackageReleasePayload {
                        format: product.format,
                        name: match product.format {
                            PackageReleasePayloadFormat::TarGz => {
                                format!("{}-{target}.tar.gz", product.id)
                            }
                        },
                        target,
                        members: sorted(product.bins.clone()),
                    })
                    .collect(),
            }
        })
        .collect::<Vec<_>>();
    products.sort_by(|left, right| left.id.cmp(&right.id));
    products
}

fn release_companions(plan: &PackageReleasePlan) -> Vec<PackageReleaseCompanion> {
    let mut companions = plan
        .companions
        .iter()
        .map(|companion| PackageReleaseCompanion {
            id: companion.id.clone(),
            name: companion.asset_name.clone(),
        })
        .collect::<Vec<_>>();
    companions.sort_by(|left, right| left.id.cmp(&right.id));
    companions
}

fn attested_subjects(artifacts: &[VerifiedPackageArtifact]) -> Vec<String> {
    let mut subjects = artifacts
        .iter()
        .filter(|artifact| {
            matches!(
                artifact.release_asset.kind,
                PackageReleaseAssetKind::Payload | PackageReleaseAssetKind::Companion,
            )
        })
        .map(|artifact| artifact.name.clone())
        .collect::<Vec<_>>();
    subjects.push("release.json".to_owned());
    subjects.sort();
    subjects
}

pub(super) fn require_expected_identity(
    receipt: &PackageReleaseReceipt,
    plan: &PackageReleasePlan,
    expected: &PackageReleaseSigningIdentity,
) -> Result<(), OrchestratorError> {
    if receipt.channel != plan.channel
        || receipt.repository.slug != expected.repository
        || receipt.repository.id != expected.repository_id
        || receipt.source.commit_sha != expected.source_sha
        || receipt.source.source_ref != expected.source_ref
        || receipt.source.default_branch != expected.default_branch
        || receipt.source.event != expected.event
        || receipt.source.ci_run_id != expected.ci_run_id
        || receipt.source.ci_run_attempt != expected.ci_run_attempt
        || receipt.workflow.authority_sha != expected.workflow_sha
        || receipt.workflow.path != expected.workflow_path
        || receipt.workflow.workflow_ref != expected.source_ref
        || receipt.workflow.run_id != expected.run_id
        || receipt.workflow.run_attempt != expected.run_attempt
        || receipt.version.commit_count != expected.commit_count
        || receipt.version.base != plan.base_version
        || receipt.version.value != expected_version(plan, expected)?
        || receipt.tag != expected_tag(plan, expected)?
        || receipt.products.len() != plan.products.len()
        || receipt.companions.len() != plan.companions.len()
    {
        return Err(internal("package_receipt_identity_mismatch"));
    }
    for (actual, declared) in receipt.products.iter().zip(&plan.products) {
        let mut targets = declared.targets.clone();
        targets.sort();
        if actual.id != declared.id
            || actual.package != declared.package
            || actual.package_version != plan.base_version
            || actual.bins != sorted(declared.bins.clone())
            || actual.payloads.len() != targets.len()
            || actual
                .payloads
                .iter()
                .zip(targets)
                .any(|(archive, target)| {
                    let expected_name = match declared.format {
                        PackageReleasePayloadFormat::TarGz => {
                            format!("{}-{target}.tar.gz", declared.id)
                        }
                    };
                    archive.format != declared.format
                        || archive.target != target
                        || archive.name != expected_name
                        || archive.members != actual.bins
                })
        {
            return Err(internal("package_receipt_product_inventory_mismatch"));
        }
    }
    for (actual, declared) in receipt.companions.iter().zip(&plan.companions) {
        if actual.id != declared.id || actual.name != declared.asset_name {
            return Err(internal("package_receipt_companion_inventory_mismatch"));
        }
    }
    Ok(())
}

fn expected_version(
    plan: &PackageReleasePlan,
    expected: &PackageReleaseSigningIdentity,
) -> Result<String, OrchestratorError> {
    match plan.channel {
        PackageReleaseChannel::Preview => {
            let identity = velnor_actions_contract::preview_identity::PreviewIdentity::derive(
                &plan.base_version,
                &expected.source_sha,
                expected
                    .commit_count
                    .ok_or_else(|| internal("package_receipt_commit_count_missing"))?,
            )
            .map_err(|_| internal("package_receipt_preview_identity_invalid"))?;
            Ok(identity.version)
        }
        PackageReleaseChannel::Stable => Ok(plan.base_version.clone()),
    }
}

fn expected_tag(
    plan: &PackageReleasePlan,
    expected: &PackageReleaseSigningIdentity,
) -> Result<String, OrchestratorError> {
    match plan.channel {
        PackageReleaseChannel::Preview => {
            let identity = velnor_actions_contract::preview_identity::PreviewIdentity::derive(
                &plan.base_version,
                &expected.source_sha,
                expected
                    .commit_count
                    .ok_or_else(|| internal("package_receipt_commit_count_missing"))?,
            )
            .map_err(|_| internal("package_receipt_preview_identity_invalid"))?;
            Ok(identity.tag)
        }
        PackageReleaseChannel::Stable => Ok(format!("v{}", plan.base_version)),
    }
}

impl VerifiedPackageArtifact {
    pub(super) fn receipt_subject(path: &std::path::Path, bytes: &[u8]) -> Self {
        let sha256 = super::runtime::sha256(bytes);
        Self {
            name: "release.json".to_owned(),
            path: path.to_path_buf(),
            size: bytes.len() as u64,
            sha256: sha256.clone(),
            release_asset: velnor_actions_contract::package_release::PackageReleaseAsset {
                name: "release.json".to_owned(),
                kind: PackageReleaseAssetKind::Payload,
                product_id: None,
                target: None,
                subject_name: Some("release.json".to_owned()),
                size: bytes.len() as u64,
                sha256,
            },
        }
    }
}

fn sorted(mut values: Vec<String>) -> Vec<String> {
    values.sort();
    values
}
