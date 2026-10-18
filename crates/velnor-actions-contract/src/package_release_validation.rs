//! Structural validation for the canonical package-release receipt.

use std::collections::BTreeSet;

use crate::{
    ContractError,
    config::{RustBinaryPreviewConfig, is_exact_semver},
    package_release_plan::PackageReleasePlan,
};

#[path = "package_release_asset_validation.rs"]
mod assets;

use super::{
    MAX_PACKAGE_RELEASE_ARCHIVES, MAX_PACKAGE_RELEASE_PRODUCTS, PACKAGE_RELEASE_EVIDENCE_PROFILE,
    PACKAGE_RELEASE_RECEIPT_BUNDLE_NAME, PACKAGE_RELEASE_RECEIPT_NAME, PACKAGE_RELEASE_SCHEMA_V2,
    PackageReleaseChannel, PackageReleaseEvent, PackageReleaseReceipt, PackageReleaseSbomFormat,
};

const SHA256SUMS_NAME: &str = "SHA256SUMS";
const OIDC_ISSUER: &str = "https://token.actions.githubusercontent.com";
const SLSA_PREDICATE: &str = "https://slsa.dev/provenance/v1";

pub(super) fn validate(receipt: &PackageReleaseReceipt) -> Result<(), ContractError> {
    if receipt.schema != PACKAGE_RELEASE_SCHEMA_V2 {
        return Err(invalid("unsupported_schema"));
    }
    validate_repository(receipt)?;
    validate_source(receipt)?;
    validate_workflow(receipt)?;
    validate_version(receipt)?;
    validate_products(receipt)?;
    validate_companions(receipt)?;
    assets::validate_assets(receipt)?;
    validate_evidence(receipt)?;
    Ok(())
}

pub(super) fn validate_against_preview(
    receipt: &PackageReleaseReceipt,
    preview: &RustBinaryPreviewConfig,
) -> Result<(), ContractError> {
    receipt.validate()?;
    preview.validate("release.json")?;
    if receipt.channel != PackageReleaseChannel::Preview || !preview.enabled {
        return Err(invalid("preview_policy_not_enabled"));
    }
    validate_against_inventory(receipt, &preview.products, &preview.companions)
}

pub(super) fn validate_against_plan(
    receipt: &PackageReleaseReceipt,
    plan: &PackageReleasePlan,
) -> Result<(), ContractError> {
    receipt.validate()?;
    plan.validate()?;
    if plan.channel != PackageReleaseChannel::Preview
        || receipt.channel != plan.channel
        || receipt.repository.slug != plan.repository
        || receipt.source.default_branch != plan.default_branch
        || receipt.version.base != plan.base_version
    {
        return Err(invalid("preview_plan_identity_mismatch"));
    }
    validate_against_inventory(receipt, &plan.products, &plan.companions)
}

fn validate_against_inventory(
    receipt: &PackageReleaseReceipt,
    declared_products: &[crate::config::RustBinaryPreviewProduct],
    declared_companions: &[crate::config::RustBinaryPreviewCompanion],
) -> Result<(), ContractError> {
    if receipt.products.len() != declared_products.len() {
        return Err(invalid("preview_product_inventory_mismatch"));
    }
    let mut sorted_products: Vec<_> = declared_products.iter().collect();
    sorted_products.sort_unstable_by(|left, right| left.id.cmp(&right.id));
    for (product, declared) in receipt.products.iter().zip(sorted_products) {
        if product.id != declared.id
            || product.package != declared.package
            || product.bins != sorted_strings(&declared.bins)
            || product.archives.len() != declared.targets.len()
        {
            return Err(invalid("preview_product_inventory_mismatch"));
        }
        let targets = sorted_strings(&declared.targets);
        for (archive, target) in product.archives.iter().zip(targets) {
            if archive.target != target
                || archive.name != format!("{}-{target}.tar.gz", declared.id)
                || archive.members != product.bins
            {
                return Err(invalid("preview_archive_inventory_mismatch"));
            }
        }
    }
    let mut expected_companions: Vec<_> = declared_companions
        .iter()
        .map(|item| (item.id.as_str(), item.asset_name.as_str()))
        .collect();
    expected_companions.sort_unstable();
    let observed_companions: Vec<_> = receipt
        .companions
        .iter()
        .map(|item| (item.id.as_str(), item.name.as_str()))
        .collect();
    if expected_companions != observed_companions {
        return Err(invalid("preview_companion_inventory_mismatch"));
    }
    let expected_names: BTreeSet<_> = expected_asset_names(declared_products, declared_companions)
        .into_iter()
        .filter(|name| {
            name != PACKAGE_RELEASE_RECEIPT_NAME && name != PACKAGE_RELEASE_RECEIPT_BUNDLE_NAME
        })
        .collect();
    let observed_names: BTreeSet<_> = receipt
        .assets
        .iter()
        .map(|asset| asset.name.clone())
        .collect();
    if expected_names != observed_names {
        return Err(invalid("preview_asset_inventory_mismatch"));
    }
    Ok(())
}

fn expected_asset_names(
    products: &[crate::config::RustBinaryPreviewProduct],
    companions: &[crate::config::RustBinaryPreviewCompanion],
) -> Vec<String> {
    let mut names = Vec::new();
    for product in products {
        for target in &product.targets {
            let archive = format!("{}-{target}.tar.gz", product.id);
            names.extend([
                archive.clone(),
                format!("{archive}.sha256"),
                format!("{archive}.bundle"),
                format!("{archive}.sbom.json"),
            ]);
        }
    }
    for companion in companions {
        names.push(companion.asset_name.clone());
        names.push(format!("{}.bundle", companion.asset_name));
    }
    names.extend([
        SHA256SUMS_NAME.to_owned(),
        PACKAGE_RELEASE_RECEIPT_NAME.to_owned(),
        PACKAGE_RELEASE_RECEIPT_BUNDLE_NAME.to_owned(),
    ]);
    names
}

fn validate_repository(receipt: &PackageReleaseReceipt) -> Result<(), ContractError> {
    if receipt.repository.id == 0
        || !crate::vcs::is_safe_github_repository(&receipt.repository.slug)
    {
        return Err(invalid("bad_repository_identity"));
    }
    Ok(())
}

fn validate_source(receipt: &PackageReleaseReceipt) -> Result<(), ContractError> {
    let source = &receipt.source;
    if !hex_string(&source.commit_sha, 40)
        || !valid_branch(&source.default_branch)
        || source.source_ref != format!("refs/heads/{}", source.default_branch)
        || source.ci_run_id == 0
        || source.ci_run_attempt == 0
        || match receipt.channel {
            PackageReleaseChannel::Preview => source.event != PackageReleaseEvent::Push,
            PackageReleaseChannel::Stable => source.event != PackageReleaseEvent::WorkflowDispatch,
        }
    {
        return Err(invalid("bad_source_identity"));
    }
    Ok(())
}

fn validate_workflow(receipt: &PackageReleaseReceipt) -> Result<(), ContractError> {
    let workflow = &receipt.workflow;
    if workflow.path != receipt.channel.workflow_path()
        || workflow.workflow_ref != receipt.source.source_ref
        || !hex_string(&workflow.authority_sha, 40)
        || workflow.authority_sha != receipt.source.commit_sha
        || workflow.run_id == 0
        || workflow.run_attempt == 0
        || workflow.producer_run_id == 0
        || workflow.producer_run_attempt == 0
        || workflow.producer_run_id != workflow.run_id
        || workflow.producer_run_attempt != workflow.run_attempt
        || workflow.signer_job_id != receipt.channel.signer_job_id()
        || workflow.signer_workflow != receipt.channel.workflow_path()
    {
        return Err(invalid("bad_workflow_identity"));
    }
    Ok(())
}

fn validate_version(receipt: &PackageReleaseReceipt) -> Result<(), ContractError> {
    let version = &receipt.version;
    if !is_exact_semver(&version.base) {
        return Err(invalid("bad_release_version_identity"));
    }
    match receipt.channel {
        PackageReleaseChannel::Preview => {
            let (Some(count), Some(sha7)) = (version.commit_count, version.source_sha7.as_deref())
            else {
                return Err(invalid("preview_version_identity_missing"));
            };
            if count == 0
                || sha7.len() != 7
                || !hex_string(sha7, 7)
                || !receipt.source.commit_sha.starts_with(sha7)
                || version.value != format!("{}-preview.{count}+{sha7}", version.base)
                || receipt.tag != format!("v{}", version.value)
            {
                return Err(invalid("version_does_not_match_preview_policy"));
            }
        }
        PackageReleaseChannel::Stable => {
            if version.commit_count.is_some()
                || version.source_sha7.is_some()
                || version.value != version.base
                || receipt.products.len() != 1
                || receipt.tag != format!("{}-v{}", receipt.products[0].package, version.base)
            {
                return Err(invalid("version_does_not_match_stable_policy"));
            }
        }
    }
    Ok(())
}

fn validate_products(receipt: &PackageReleaseReceipt) -> Result<(), ContractError> {
    if receipt.products.is_empty() || receipt.products.len() > MAX_PACKAGE_RELEASE_PRODUCTS {
        return Err(invalid("invalid_product_count"));
    }
    if !sorted_unique(receipt.products.iter().map(|product| product.id.as_str())) {
        return Err(invalid("products_not_sorted_unique"));
    }
    let mut archive_count = 0;
    for product in &receipt.products {
        if !valid_identifier(&product.id)
            || !valid_identifier(&product.package)
            || product.package_version != receipt.version.base
            || product.bins.is_empty()
            || product.bins.iter().any(|bin| !valid_identifier(bin))
            || !sorted_unique(product.bins.iter().map(String::as_str))
        {
            return Err(invalid("bad_product_identity"));
        }
        if product.archives.is_empty()
            || !sorted_unique(
                product
                    .archives
                    .iter()
                    .map(|archive| archive.target.as_str()),
            )
        {
            return Err(invalid("archives_not_sorted_unique"));
        }
        for archive in &product.archives {
            archive_count += 1;
            let name = format!("{}-{}.tar.gz", product.id, archive.target);
            if !crate::package_release::is_supported_release_target(&archive.target)
                || archive.name != name
                || !valid_asset_name(&archive.name)
                || archive.members != product.bins
            {
                return Err(invalid("bad_archive_identity_or_members"));
            }
        }
    }
    if archive_count > MAX_PACKAGE_RELEASE_ARCHIVES {
        return Err(invalid("too_many_archives"));
    }
    Ok(())
}

fn validate_companions(receipt: &PackageReleaseReceipt) -> Result<(), ContractError> {
    if !sorted_unique(receipt.companions.iter().map(|item| item.id.as_str())) {
        return Err(invalid("companions_not_sorted_unique"));
    }
    let product_ids: BTreeSet<_> = receipt
        .products
        .iter()
        .map(|item| item.id.as_str())
        .collect();
    let mut names = BTreeSet::new();
    for companion in &receipt.companions {
        if !valid_identifier(&companion.id)
            || product_ids.contains(companion.id.as_str())
            || !valid_asset_name(&companion.name)
            || companion.name == SHA256SUMS_NAME
            || companion.name == PACKAGE_RELEASE_RECEIPT_NAME
            || companion.name == PACKAGE_RELEASE_RECEIPT_BUNDLE_NAME
            || !names.insert(companion.name.as_str())
        {
            return Err(invalid("bad_companion_identity"));
        }
    }
    Ok(())
}

fn validate_evidence(receipt: &PackageReleaseReceipt) -> Result<(), ContractError> {
    let evidence = &receipt.evidence;
    if evidence.profile != PACKAGE_RELEASE_EVIDENCE_PROFILE
        || evidence.sbom_format != PackageReleaseSbomFormat::CycloneDxJson
        || evidence.signer_issuer != OIDC_ISSUER
        || evidence.attestation_predicate != SLSA_PREDICATE
    {
        return Err(invalid("unsupported_release_evidence"));
    }
    let expected_ref = format!(
        "{}/{}@{}",
        receipt.repository.slug, receipt.workflow.path, receipt.workflow.workflow_ref
    );
    if evidence.signer_workflow_ref != expected_ref {
        return Err(invalid("signer_workflow_identity_mismatch"));
    }
    let mut expected_subjects: Vec<_> = receipt
        .products
        .iter()
        .flat_map(|product| product.archives.iter().map(|archive| archive.name.clone()))
        .chain(receipt.companions.iter().map(|item| item.name.clone()))
        .collect();
    expected_subjects.push(PACKAGE_RELEASE_RECEIPT_NAME.to_owned());
    expected_subjects.sort();
    if evidence.attested_subject_names != expected_subjects {
        return Err(invalid("attestation_subject_inventory_mismatch"));
    }
    Ok(())
}

fn sorted_strings(values: &[String]) -> Vec<String> {
    let mut sorted = values.to_vec();
    sorted.sort();
    sorted
}

pub(super) fn sorted_unique<'a>(values: impl Iterator<Item = &'a str>) -> bool {
    let mut previous = None;
    for value in values {
        if previous.is_some_and(|prior| prior >= value) {
            return false;
        }
        previous = Some(value);
    }
    true
}

fn valid_branch(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 255
        && !value.starts_with('/')
        && !value.ends_with('/')
        && !value.contains("..")
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'/' | b'.'))
}

fn valid_identifier(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 100
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
}

pub(super) fn valid_asset_name(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 180
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
        && !matches!(value, "." | "..")
}

pub(super) fn hex_string(value: &str, len: usize) -> bool {
    value.len() == len
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

pub(super) fn invalid(problem: &str) -> ContractError {
    ContractError::identity("package_release", problem)
}
