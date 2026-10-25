use serde::{Deserialize, Serialize};

use crate::{ContractError, canonical::canonical_json_bytes};

#[path = "package_release_event.rs"]
mod event;
#[path = "package_release_parse.rs"]
mod parse;
pub use event::{NativePreviewEvent, PackageReleaseEvent};

#[path = "package_release_validation.rs"]
mod validation;
pub(crate) use validation::valid_asset_name;

pub const PACKAGE_RELEASE_SCHEMA_V2: &str = "velnor.package-release.v2";

pub const PACKAGE_RELEASE_RECEIPT_NAME: &str = "release.json";

pub const PACKAGE_RELEASE_RECEIPT_BUNDLE_NAME: &str = "release.json.bundle";

pub const PACKAGE_RELEASE_EVIDENCE_PROFILE: &str = "signed-sbom-v1";

pub(crate) const RELEASE_TARGETS: [&str; 4] = [
    "aarch64-apple-darwin",
    "x86_64-apple-darwin",
    "aarch64-unknown-linux-gnu",
    "x86_64-unknown-linux-gnu",
];

pub(crate) const MAX_PACKAGE_RELEASE_PRODUCTS: usize = 64;

pub(crate) const MAX_PACKAGE_RELEASE_ARCHIVES: usize = 256;

pub(crate) const MAX_PACKAGE_RELEASE_ASSETS: usize = 2048;

#[must_use]
pub(crate) fn is_supported_release_target(value: &str) -> bool {
    RELEASE_TARGETS.contains(&value)
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PackageReleaseReceipt {
    pub schema: String,

    pub channel: PackageReleaseChannel,

    pub repository: PackageReleaseRepository,

    pub source: PackageReleaseSource,

    pub workflow: PackageReleaseWorkflow,

    pub version: PackageReleaseVersion,

    pub tag: String,

    pub products: Vec<PackageReleaseProduct>,

    pub companions: Vec<PackageReleaseCompanion>,

    pub assets: Vec<PackageReleaseAsset>,

    pub evidence: PackageReleaseEvidence,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PackageReleaseChannel {
    Stable,

    Preview,
}

impl PackageReleaseChannel {
    #[must_use]
    pub const fn workflow_path(self) -> &'static str {
        match self {
            Self::Preview => ".github/workflows/binary-preview.yml",
            Self::Stable => ".github/workflows/binary-release.yml",
        }
    }

    #[must_use]
    pub const fn assemble_job_id(self) -> &'static str {
        match self {
            Self::Preview => "preview-assemble",
            Self::Stable => "stable-assemble",
        }
    }

    #[must_use]
    pub const fn signer_job_id(self) -> &'static str {
        match self {
            Self::Preview => "preview-signer",
            Self::Stable => "stable-signer",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PackageReleaseRepository {
    pub slug: String,

    pub id: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PackageReleaseSource {
    pub commit_sha: String,

    #[serde(rename = "ref")]
    pub source_ref: String,

    pub default_branch: String,

    pub event: PackageReleaseEvent,

    pub ci_run_id: u64,

    pub ci_run_attempt: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PackageReleaseWorkflow {
    pub path: String,

    #[serde(rename = "ref")]
    pub workflow_ref: String,

    pub authority_sha: String,

    pub run_id: u64,

    pub run_attempt: u32,

    pub producer_run_id: u64,

    pub producer_run_attempt: u32,

    pub signer_job_id: String,

    pub signer_workflow: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PackageReleaseVersion {
    pub base: String,

    pub value: String,

    pub commit_count: Option<u64>,

    pub source_sha7: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PackageReleaseProduct {
    pub id: String,

    pub package: String,

    pub package_version: String,

    pub bins: Vec<String>,

    pub payloads: Vec<PackageReleasePayload>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PackageReleasePayload {
    pub format: PackageReleasePayloadFormat,

    pub target: String,

    pub name: String,

    pub members: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PackageReleasePayloadFormat {
    TarGz,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PackageReleaseCompanion {
    pub id: String,

    pub name: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PackageReleaseAssetKind {
    Payload,

    Sha256,

    SignatureBundle,

    Sbom,

    ChecksumList,

    Companion,

    CompanionSignatureBundle,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PackageReleaseAsset {
    pub name: String,

    pub kind: PackageReleaseAssetKind,

    pub product_id: Option<String>,

    pub target: Option<String>,

    pub subject_name: Option<String>,

    pub size: u64,

    pub sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PackageReleaseEvidence {
    pub profile: String,

    pub sbom_format: PackageReleaseSbomFormat,

    pub signer_issuer: String,

    pub signer_workflow_ref: String,

    pub attestation_predicate: String,

    pub attested_subject_names: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PackageReleaseSbomFormat {
    #[serde(rename = "CycloneDX-JSON")]
    CycloneDxJson,
}

impl PackageReleaseReceipt {
    pub fn validate(&self) -> Result<(), ContractError> {
        validation::validate(self)
    }

    pub fn validate_against_config(
        &self,
        config: &crate::config::RustBinaryReleaseConfig,
    ) -> Result<(), ContractError> {
        validation::validate_against_config(self, config)
    }

    pub fn validate_against_plan(
        &self,
        plan: &crate::package_release_plan::PackageReleasePlan,
    ) -> Result<(), ContractError> {
        validation::validate_against_plan(self, plan)
    }

    pub fn canonical_bytes(&self) -> Result<Vec<u8>, ContractError> {
        canonical_json_bytes(self)
    }

    pub fn canonical_digest(&self) -> Result<String, ContractError> {
        Ok(crate::canonical::digest_b3(&self.canonical_bytes()?))
    }

    #[must_use]
    pub fn asset_identities(&self) -> Vec<PackageReleaseAssetIdentity> {
        self.assets
            .iter()
            .map(|asset| PackageReleaseAssetIdentity {
                name: asset.name.clone(),
                size: asset.size,
                sha256: asset.sha256.clone(),
            })
            .collect()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PackageReleaseAssetIdentity {
    pub name: String,

    pub size: u64,

    pub sha256: String,
}

pub use parse::parse_package_release_receipt;
