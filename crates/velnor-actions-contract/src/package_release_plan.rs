//! Static, credential-free input contract for native package release jobs.

use serde::{Deserialize, Serialize};

use crate::config::{RustBinaryPreviewArchive, RustBinaryPreviewProduct};
use crate::package_release::{PackageReleaseChannel, PackageReleasePayloadFormat};
use crate::{
    ContractError, canonical_json_bytes, config::RustBinaryPreviewCompanion, is_exact_semver,
    is_valid_branch_name,
};

/// Static producer request schema. It contains inventory only, never task code.
pub const PACKAGE_RELEASE_PLAN_SCHEMA_V1: &str = "velnor.package-release-plan.v1";

/// Source-bound static plan passed from workflow authority to unprivileged producer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PackageReleasePlan {
    /// Closed static-plan schema discriminator.
    pub schema: String,
    /// Release channel selected by the generated workflow.
    pub channel: PackageReleaseChannel,
    /// Exact owner/repository slug from workflow context.
    pub repository: String,
    /// Configured default branch, without `refs/heads/` prefix.
    pub default_branch: String,
    /// Shared numeric Cargo package base version.
    pub base_version: String,
    /// Literal product inventory copied from source policy.
    pub products: Vec<PackageReleasePlanProduct>,
    /// Literal companion inventory copied from source policy.
    pub companions: Vec<RustBinaryPreviewCompanion>,
}

/// Channel-neutral product matrix carried to isolated release jobs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PackageReleasePlanProduct {
    /// Stable product identity.
    pub id: String,
    /// Exact Cargo package name.
    pub package: String,
    /// Exact executable targets in every payload.
    pub bins: Vec<String>,
    /// Rust target triples.
    pub targets: Vec<String>,
    /// Actual byte representation; never inferred from a filename.
    pub format: PackageReleasePayloadFormat,
}

impl PackageReleasePlan {
    /// Validate this static plan independently from any source configuration.
    /// # Errors
    /// Rejects unknown schema, malformed repository/branch/version, or empty products.
    pub fn validate(&self) -> Result<(), ContractError> {
        if self.schema != PACKAGE_RELEASE_PLAN_SCHEMA_V1 {
            return Err(invalid("unknown_schema"));
        }
        if !crate::vcs::is_safe_github_repository(&self.repository) {
            return Err(invalid("bad_repository"));
        }
        if !is_valid_branch_name(&self.default_branch) {
            return Err(invalid("bad_default_branch"));
        }
        if self.channel == PackageReleaseChannel::Preview && self.default_branch != "main" {
            return Err(invalid("preview_requires_main_default_branch"));
        }
        if !is_exact_semver(&self.base_version) {
            return Err(invalid("base_version_must_be_numeric"));
        }
        if self.products.is_empty() {
            return Err(invalid("empty_product_inventory"));
        }
        if self
            .products
            .windows(2)
            .any(|pair| pair[0].id >= pair[1].id)
            || self
                .companions
                .windows(2)
                .any(|pair| pair[0].id >= pair[1].id)
            || self.products.iter().any(|product| {
                product.bins.windows(2).any(|pair| pair[0] >= pair[1])
                    || product.targets.windows(2).any(|pair| pair[0] >= pair[1])
            })
        {
            return Err(invalid("inventory_not_canonically_sorted"));
        }
        let config_products = self
            .products
            .iter()
            .map(|product| RustBinaryPreviewProduct {
                id: product.id.clone(),
                package: product.package.clone(),
                bins: product.bins.clone(),
                targets: product.targets.clone(),
                archive: RustBinaryPreviewArchive::TarGz,
            })
            .collect::<Vec<_>>();
        crate::config::validate_static_release_inventory(
            "package-release-plan",
            &config_products,
            &self.companions,
            None,
        )?;
        for product in &self.products {
            if (self.channel == PackageReleaseChannel::Preview
                && product.format != PackageReleasePayloadFormat::TarGz)
                || (self.channel == PackageReleaseChannel::Stable
                    && (product.format != PackageReleasePayloadFormat::RawBinary
                        || product.bins.len() != 1
                        || product.targets.len() != 1))
            {
                return Err(invalid("product_inventory_invalid_for_channel"));
            }
        }
        Ok(())
    }

    /// Canonical JSON bytes for workflow request files and output proofs.
    /// # Errors
    /// Returns an encoding error if canonical JSON cannot be represented.
    pub fn canonical_bytes(&self) -> Result<Vec<u8>, ContractError> {
        self.validate()?;
        canonical_json_bytes(self)
    }
}

/// Parse a strict canonical static plan from untrusted bytes.
/// # Errors
/// Rejects duplicate keys, unknown fields, invalid UTF-8, oversized/noncanonical bytes.
pub fn parse_package_release_plan(bytes: &[u8]) -> Result<PackageReleasePlan, ContractError> {
    let value = crate::strict_json::parse_strict_json_bytes(bytes, 1024 * 1024)?;
    let plan: PackageReleasePlan = serde_json::from_value(value)
        .map_err(|error| ContractError::identity("package_release_plan", error.to_string()))?;
    plan.validate()?;
    if plan.canonical_bytes()?.as_slice() != bytes {
        return Err(invalid("noncanonical_json"));
    }
    Ok(plan)
}

fn invalid(problem: &'static str) -> ContractError {
    ContractError::identity("package_release_plan", problem)
}

#[cfg(test)]
#[path = "package_release_plan_tests.rs"]
mod tests;
