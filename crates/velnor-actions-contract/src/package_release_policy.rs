// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::config::{
    RustBinaryReleaseArchive, RustBinaryReleaseCompanion, RustBinaryReleaseProduct,
};
use crate::package_release::PackageReleaseChannel;
use crate::{ContractError, PackageReleasePlan, PackageReleasePlanProduct, canonical_json_bytes};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

pub const PACKAGE_RELEASE_POLICY_SCHEMA_V1: &str = "velnor.package-release-policy.v1";

pub const MAX_CARGO_VERSION_MANIFEST_BYTES: usize = 1024 * 1024;

pub const MAX_CARGO_VERSION_INPUT_BYTES: usize = 8 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PackageReleasePolicy {
    pub schema: String,

    pub channel: PackageReleaseChannel,

    pub repository: String,

    pub default_branch: String,

    pub version: CargoVersionPolicy,

    pub products: Vec<PackageReleasePlanProduct>,

    pub companions: Vec<RustBinaryReleaseCompanion>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CargoVersionPolicy {
    pub version_package: String,

    pub bindings: Vec<CargoVersionBinding>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CargoVersionBinding {
    pub package: String,

    pub manifest_path: String,

    pub workspace_manifest_path: String,
}
impl PackageReleasePolicy {
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

    pub fn canonical_bytes(&self) -> Result<Vec<u8>, ContractError> {
        self.validate()?;
        canonical_json_bytes(self)
    }
}
impl CargoVersionPolicy {
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
