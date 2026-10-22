//! Static release ownership and inventory; versions resolve only from exact source.
use crate::config::{
    RustBinaryReleaseArchive, RustBinaryReleaseCompanion, RustBinaryReleaseProduct,
};
use crate::package_release::PackageReleaseChannel;
use crate::{ContractError, PackageReleasePlan, PackageReleasePlanProduct, canonical_json_bytes};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
/// Generated inventory policy schema; never a partially resolved plan.
pub const PACKAGE_RELEASE_POLICY_SCHEMA_V1: &str = "velnor.package-release-policy.v1";
/// Maximum authenticated TOML bytes for one version input.
pub const MAX_CARGO_VERSION_MANIFEST_BYTES: usize = 1024 * 1024;
/// Maximum complete manifest-data closure accepted by version resolution.
pub const MAX_CARGO_VERSION_INPUT_BYTES: usize = 8 * 1024 * 1024;
/// Literal source policy with no generation-time Cargo version value.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PackageReleasePolicy {
    /// Closed schema discriminator.
    pub schema: String,
    /// Native channel.
    pub channel: PackageReleaseChannel,
    /// Source repository slug.
    pub repository: String,
    /// Protected default branch.
    pub default_branch: String,
    /// Discovered package and manifest ownership, independent of version values.
    pub version: CargoVersionPolicy,
    /// Shared sorted product inventory.
    pub products: Vec<PackageReleasePlanProduct>,
    /// Shared sorted companion inventory.
    pub companions: Vec<RustBinaryReleaseCompanion>,
}
/// One authoritative version owner and the complete product-package closure.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CargoVersionPolicy {
    /// Exact product package owning the numeric base version.
    pub version_package: String,
    /// Distinct product packages sorted by package name.
    pub bindings: Vec<CargoVersionBinding>,
}
/// Data-only Cargo manifest paths captured from discovered workspace membership.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CargoVersionBinding {
    /// Expected package.name, never inferred from filename.
    pub package: String,
    /// Exact regular package Cargo.toml path.
    pub manifest_path: String,
    /// Exact owning workspace Cargo.toml for workspace.package.version.
    pub workspace_manifest_path: String,
}
impl PackageReleasePolicy {
    /// Validate the closed static inventory and complete ownership closure.
    /// # Errors
    /// Rejects malformed identities, paths, sorting or incomplete package coverage.
    pub fn validate(&self) -> Result<(), ContractError> {
        if self.schema != PACKAGE_RELEASE_POLICY_SCHEMA_V1
            || !crate::vcs::is_safe_github_repository(&self.repository)
            || !crate::is_valid_branch_name(&self.default_branch)
            || (self.channel == PackageReleaseChannel::Preview && self.default_branch != "main")
        {
            return Err(invalid("invalid_policy_identity"));
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
                product.format != crate::package_release::PackageReleasePayloadFormat::TarGz
                    || product.bins.windows(2).any(|pair| pair[0] >= pair[1])
                    || product.targets.windows(2).any(|pair| pair[0] >= pair[1])
            })
        {
            return Err(invalid("noncanonical_inventory"));
        }
        let products = self
            .products
            .iter()
            .map(|product| RustBinaryReleaseProduct {
                id: product.id.clone(),
                package: product.package.clone(),
                bins: product.bins.clone(),
                targets: product.targets.clone(),
                archive: RustBinaryReleaseArchive::TarGz,
            })
            .collect::<Vec<_>>();
        crate::config::validate_static_release_inventory(
            "package-release-policy",
            &products,
            &self.companions,
            None,
        )?;
        let packages = self
            .products
            .iter()
            .map(|product| product.package.as_str())
            .collect::<BTreeSet<_>>();
        self.version.validate(&packages)
    }
    /// Exact sorted regular Git-file paths the version resolver requires.
    #[must_use]
    pub fn manifest_paths(&self) -> Vec<&str> {
        self.version
            .bindings
            .iter()
            .flat_map(|binding| {
                [
                    binding.manifest_path.as_str(),
                    binding.workspace_manifest_path.as_str(),
                ]
            })
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect()
    }
    /// Canonical bounded source-policy pair inventory, independent of version values.
    /// # Errors
    /// Rejects invalid policy or an inventory exceeding the native graph bound.
    pub fn payload_pairs(
        &self,
    ) -> Result<Vec<crate::preview_production::ReleasePayloadPair>, ContractError> {
        self.validate()?;
        let pairs = self
            .products
            .iter()
            .flat_map(|product| {
                product.targets.iter().map(move |target| {
                    crate::preview_production::ReleasePayloadPair {
                        product_id: product.id.clone(),
                        target: target.clone(),
                    }
                })
            })
            .collect::<Vec<_>>();
        if pairs.len() > crate::preview_production::MAX_RELEASE_PAYLOAD_PAIRS {
            return Err(invalid("payload_pair_limit"));
        }
        Ok(pairs)
    }
    /// Literal channel job ID for a member of the complete static inventory.
    /// Preview uses one shared ID; stable uses the member's canonical index.
    /// # Errors
    /// Rejects pairs outside the policy's bounded canonical inventory.
    pub fn payload_job_id(
        &self,
        pair: &crate::preview_production::ReleasePayloadPair,
    ) -> Result<String, ContractError> {
        let index = self
            .payload_pairs()?
            .binary_search(pair)
            .map_err(|_| invalid("unknown_payload_pair"))?;
        match self.channel {
            PackageReleaseChannel::Preview => Ok("preview-payload".to_owned()),
            PackageReleaseChannel::Stable => Ok(format!("stable-payload-{index:03}")),
        }
    }
    /// Construct a fully validated resolved plan; this pure method proves no source authority.
    /// # Errors
    /// Rejects invalid policy or a nonnumeric resolved version.
    pub fn resolve(&self, base_version: String) -> Result<PackageReleasePlan, ContractError> {
        self.validate()?;
        let plan = PackageReleasePlan {
            schema: crate::PACKAGE_RELEASE_PLAN_SCHEMA_V1.to_owned(),
            channel: self.channel,
            repository: self.repository.clone(),
            default_branch: self.default_branch.clone(),
            base_version,
            products: self.products.clone(),
            companions: self.companions.clone(),
        };
        plan.validate()?;
        Ok(plan)
    }
    /// Match every static dimension of a resolved plan, independently of version data.
    /// # Errors
    /// Rejects invalid policy/plan or substituted inventory/channel/repository.
    pub fn validate_plan(&self, plan: &PackageReleasePlan) -> Result<(), ContractError> {
        self.validate()?;
        plan.validate()?;
        if self.channel != plan.channel
            || self.repository != plan.repository
            || self.default_branch != plan.default_branch
            || self.products != plan.products
            || self.companions != plan.companions
        {
            return Err(invalid("resolved_plan_policy_mismatch"));
        }
        Ok(())
    }
    /// Deterministic generated policy bytes.
    /// # Errors
    /// Rejects invalid policy or canonical encoding failure.
    pub fn canonical_bytes(&self) -> Result<Vec<u8>, ContractError> {
        self.validate()?;
        canonical_json_bytes(self)
    }
}
impl CargoVersionPolicy {
    /// Validate complete manifest ownership against the shared product packages.
    /// # Errors
    /// Rejects missing/extra/duplicate packages, ambiguous files or unsafe paths.
    pub fn validate(&self, packages: &BTreeSet<&str>) -> Result<(), ContractError> {
        if !packages.contains(self.version_package.as_str())
            || self
                .bindings
                .windows(2)
                .any(|pair| pair[0].package >= pair[1].package)
            || self
                .bindings
                .iter()
                .map(|binding| binding.package.as_str())
                .collect::<BTreeSet<_>>()
                != *packages
        {
            return Err(invalid("version_package_coverage"));
        }
        let mut manifests = BTreeSet::new();
        let mut workspace = None;
        for binding in &self.bindings {
            if !safe_manifest(&binding.manifest_path)
                || !safe_manifest(&binding.workspace_manifest_path)
                || !manifests.insert(&binding.manifest_path)
            {
                return Err(invalid("version_manifest_path"));
            }
            if workspace
                .replace(&binding.workspace_manifest_path)
                .is_some_and(|prior| prior != &binding.workspace_manifest_path)
            {
                return Err(invalid("version_workspace_ownership"));
            }
        }
        Ok(())
    }
}
fn safe_manifest(path: &str) -> bool {
    path.len() <= 4096
        && (path == "Cargo.toml" || path.ends_with("/Cargo.toml"))
        && !path.starts_with('/')
        && !path
            .split('/')
            .any(|part| part.is_empty() || part == "." || part == "..")
        && path
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'/' | b'.' | b'_' | b'-'))
}
/// Parse one canonical closed policy; no fallback to resolved-plan parsing.
/// # Errors
/// Rejects duplicates, unknown fields, oversized bytes and noncanonical encoding.
pub fn parse_package_release_policy(bytes: &[u8]) -> Result<PackageReleasePolicy, ContractError> {
    let value = crate::strict_json::parse_strict_json_bytes(bytes, 1024 * 1024)?;
    let policy: PackageReleasePolicy = serde_json::from_value(value)
        .map_err(|error| ContractError::identity("package_release_policy", error.to_string()))?;
    if policy.canonical_bytes()?.as_slice() != bytes {
        return Err(invalid("noncanonical_policy"));
    }
    Ok(policy)
}
fn invalid(reason: &'static str) -> ContractError {
    ContractError::identity("package_release_policy", reason)
}
#[cfg(test)]
#[path = "package_release_policy_tests.rs"]
mod tests;
