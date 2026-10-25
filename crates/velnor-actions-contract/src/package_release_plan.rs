use serde::{Deserialize, Serialize};

use crate::config::{RustBinaryReleaseArchive, RustBinaryReleaseProduct};
use crate::package_release::{PackageReleaseChannel, PackageReleasePayloadFormat};
use crate::{
    ContractError, canonical_json_bytes, config::RustBinaryReleaseCompanion, is_exact_semver,
    is_valid_branch_name,
};

pub const PACKAGE_RELEASE_PLAN_SCHEMA_V1: &str = "velnor.package-release-plan.v1";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PackageReleasePlan {
    pub schema: String,

    pub channel: PackageReleaseChannel,

    pub repository: String,

    pub default_branch: String,

    pub base_version: String,

    pub products: Vec<PackageReleasePlanProduct>,

    pub companions: Vec<RustBinaryReleaseCompanion>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PackageReleasePlanProduct {
    pub id: String,

    pub package: String,

    pub bins: Vec<String>,

    pub targets: Vec<String>,

    pub format: PackageReleasePayloadFormat,
}

impl PackageReleasePlan {
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
            .map(|product| RustBinaryReleaseProduct {
                id: product.id.clone(),
                package: product.package.clone(),
                bins: product.bins.clone(),
                targets: product.targets.clone(),
                archive: RustBinaryReleaseArchive::TarGz,
            })
            .collect::<Vec<_>>();
        crate::config::validate_static_release_inventory(
            "package-release-plan",
            &config_products,
            &self.companions,
            None,
        )?;
        Ok(())
    }

    pub fn canonical_bytes(&self) -> Result<Vec<u8>, ContractError> {
        self.validate()?;
        canonical_json_bytes(self)
    }
}

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
