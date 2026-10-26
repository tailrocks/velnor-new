// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;

use crate::{
    ContractError, config::RustBinaryReleaseConfig, package_release_plan::PackageReleasePlan,
};

#[path = "package_release_asset_validation.rs"]
mod assets;
#[path = "package_release_identity_validation.rs"]
mod identity;

use super::{
    MAX_PACKAGE_RELEASE_ARCHIVES, MAX_PACKAGE_RELEASE_PRODUCTS, PACKAGE_RELEASE_EVIDENCE_PROFILE,
    PACKAGE_RELEASE_RECEIPT_BUNDLE_NAME, PACKAGE_RELEASE_RECEIPT_NAME, PACKAGE_RELEASE_SCHEMA_V2,
    PackageReleaseChannel, PackageReleasePayloadFormat, PackageReleaseReceipt,
    PackageReleaseSbomFormat,
};

const SHA256SUMS_NAME: &str = "SHA256SUMS";
const OIDC_ISSUER: &str = "https://token.actions.githubusercontent.com";
const SLSA_PREDICATE: &str = "https://slsa.dev/provenance/v1";

pub(super) fn validate(receipt: &PackageReleaseReceipt) -> Result<(), ContractError> {
    if receipt.schema != PACKAGE_RELEASE_SCHEMA_V2 {
        return Err(invalid("unsupported_schema"));
    }
    validate_identity(receipt)?;
    validate_products(receipt)?;
    validate_companions(receipt)?;
    assets::validate_assets(receipt)?;
    validate_evidence(receipt)?;
    Ok(())
}

pub(super) fn validate_against_config(
    receipt: &PackageReleaseReceipt,
    config: &RustBinaryReleaseConfig,
) -> Result<(), ContractError> {
    receipt.validate()?;
    config.validate("release.json")?;
    let enabled = match receipt.channel {
        PackageReleaseChannel::Preview => config.preview.enabled,
        PackageReleaseChannel::Stable => config.stable.enabled,
    };
    if !enabled {
        return Err(invalid("release_channel_not_enabled"));
    }
    validate_against_inventory(receipt, &config.products, &config.companions)
}

pub(super) fn validate_against_plan(
    receipt: &PackageReleaseReceipt,
    plan: &PackageReleasePlan,
) -> Result<(), ContractError> {
    receipt.validate()?;
    plan.validate()?;
    if receipt.channel != plan.channel
        || receipt.repository.slug != plan.repository
        || receipt.source.default_branch != plan.default_branch
        || receipt.version.base != plan.base_version
    {
        return Err(invalid("preview_plan_identity_mismatch"));
    }
    if receipt.products.len() != plan.products.len()
        || receipt.companions.len() != plan.companions.len()
    {
        return Err(invalid("plan_inventory_mismatch"));
    }
    for (actual, declared) in receipt.products.iter().zip(&plan.products) {
        if actual.id != declared.id
            || actual.package != declared.package
            || actual.bins != declared.bins
            || actual.package_version != plan.base_version
            || actual.payloads.len() != declared.targets.len()
        {
            return Err(invalid("plan_product_inventory_mismatch"));
        }
        for (archive, target) in actual.payloads.iter().zip(&declared.targets) {
            let expected_name = match declared.format {
                PackageReleasePayloadFormat::TarGz => format!("{}-{target}.tar.gz", declared.id),
            };
            if archive.format != declared.format
                || archive.target != *target
                || archive.name != expected_name
                || archive.members != actual.bins
            {
                return Err(invalid("plan_archive_inventory_mismatch"));
            }
        }
    }
    let mut expected_companions: Vec<_> = plan
        .companions
        .iter()
        .map(|item| (item.id.as_str(), item.asset_name.as_str()))
        .collect();
    expected_companions.sort_unstable();
    let actual_companions: Vec<_> = receipt
        .companions
        .iter()
        .map(|item| (item.id.as_str(), item.name.as_str()))
        .collect();
    if actual_companions != expected_companions {
        return Err(invalid("plan_companion_inventory_mismatch"));
    }
    Ok(())
}

fn validate_against_inventory(
    receipt: &PackageReleaseReceipt,
    declared_products: &[crate::config::RustBinaryReleaseProduct],
    declared_companions: &[crate::config::RustBinaryReleaseCompanion],
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
            || product.payloads.len() != declared.targets.len()
        {
            return Err(invalid("preview_product_inventory_mismatch"));
        }
        let targets = sorted_strings(&declared.targets);
        for (archive, target) in product.payloads.iter().zip(targets) {
            if archive.format != PackageReleasePayloadFormat::TarGz
                || archive.target != target
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
    products: &[crate::config::RustBinaryReleaseProduct],
    companions: &[crate::config::RustBinaryReleaseCompanion],
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

fn validate_identity(receipt: &PackageReleaseReceipt) -> Result<(), ContractError> {
    identity::validate(receipt)
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
        if product.payloads.is_empty()
            || !sorted_unique(
                product
                    .payloads
                    .iter()
                    .map(|archive| archive.target.as_str()),
            )
        {
            return Err(invalid("archives_not_sorted_unique"));
        }
        for archive in &product.payloads {
            archive_count += 1;
            let name_matches = match archive.format {
                PackageReleasePayloadFormat::TarGz => {
                    archive.name == format!("{}-{}.tar.gz", product.id, archive.target)
                }
            };
            if !crate::package_release::is_supported_release_target(&archive.target)
                || !name_matches
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
        .flat_map(|product| product.payloads.iter().map(|archive| archive.name.clone()))
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

fn valid_identifier(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 100
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
}

pub(crate) fn valid_asset_name(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 180
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
        && !value.starts_with('.')
        && !value.ends_with('.')
}

pub(super) fn invalid(problem: &str) -> ContractError {
    ContractError::identity("package_release", problem)
}
