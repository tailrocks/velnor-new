//! Canonical Velnor artifact-set release receipt, version 2.

use serde::{Deserialize, Serialize};

use crate::{ContractError, canonical::canonical_json_bytes};

#[path = "package_release_parse.rs"]
mod parse;
#[cfg(test)]
#[path = "package_release_tests.rs"]
pub(crate) mod tests;
#[path = "package_release_validation.rs"]
mod validation;

/// Receipt schema discriminator.
pub const PACKAGE_RELEASE_SCHEMA_V2: &str = "velnor.package-release.v2";
/// Canonical public receipt asset name.
pub const PACKAGE_RELEASE_RECEIPT_NAME: &str = "release.json";
/// Detached signature bundle asset name.
pub const PACKAGE_RELEASE_RECEIPT_BUNDLE_NAME: &str = "release.json.bundle";
/// Closed evidence profile name.
pub const PACKAGE_RELEASE_EVIDENCE_PROFILE: &str = "signed-sbom-v1";

/// Fixed Rust targets admitted by preview configuration and release receipts.
pub(crate) const RELEASE_TARGETS: [&str; 4] = [
    "aarch64-apple-darwin",
    "x86_64-apple-darwin",
    "aarch64-unknown-linux-gnu",
    "x86_64-unknown-linux-gnu",
];
/// Maximum products accepted by a release receipt and its source policy.
pub(crate) const MAX_PACKAGE_RELEASE_PRODUCTS: usize = 64;
/// Maximum archives accepted by a release receipt and its source policy.
pub(crate) const MAX_PACKAGE_RELEASE_ARCHIVES: usize = 256;
/// Maximum listed assets accepted by a release receipt and its source policy.
pub(crate) const MAX_PACKAGE_RELEASE_ASSETS: usize = 2048;

/// Check membership in the single shared native release target allowlist.
#[must_use]
pub(crate) fn is_supported_release_target(value: &str) -> bool {
    RELEASE_TARGETS.contains(&value)
}

/// Full immutable source, workflow, version, and asset identity.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PackageReleaseReceipt {
    /// Exact versioned schema discriminator.
    pub schema: String,
    /// Stable or preview release channel.
    pub channel: PackageReleaseChannel,
    /// Immutable source repository identity.
    pub repository: PackageReleaseRepository,
    /// Exact source commit and successful CI run identity.
    pub source: PackageReleaseSource,
    /// Authenticated release workflow and build run identity.
    pub workflow: PackageReleaseWorkflow,
    /// Base package version and deterministic channel version.
    pub version: PackageReleaseVersion,
    /// Immutable channel tag: preview `v<version.value>`, stable `<package>-v<base>`.
    pub tag: String,
    /// Product inventories sorted by product ID.
    pub products: Vec<PackageReleaseProduct>,
    /// Non-payload companions sorted by companion ID.
    pub companions: Vec<PackageReleaseCompanion>,
    /// Exact asset set excluding this receipt and its signature bundle.
    pub assets: Vec<PackageReleaseAsset>,
    /// Closed signer, SBOM, and attestation evidence declaration.
    pub evidence: PackageReleaseEvidence,
}

/// Supported immutable release channel.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PackageReleaseChannel {
    /// Human-approved stable release.
    Stable,
    /// Exact-main automatic preview.
    Preview,
}

impl PackageReleaseChannel {
    /// Canonical generated workflow that owns this release channel.
    #[must_use]
    pub const fn workflow_path(self) -> &'static str {
        match self {
            Self::Preview => ".github/workflows/binary-preview.yml",
            Self::Stable => ".github/workflows/binary-release.yml",
        }
    }

    /// Canonical generated job that holds the channel's signing authority.
    #[must_use]
    pub const fn signer_job_id(self) -> &'static str {
        match self {
            Self::Preview => "preview-signer",
            Self::Stable => "attest-binary",
        }
    }
}

/// GitHub repository slug and immutable numeric identity.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PackageReleaseRepository {
    /// Registered-case `owner/repository` slug, authenticated with its numeric ID.
    pub slug: String,
    /// GitHub repository database ID.
    pub id: u64,
}

/// Source commit identity and exact successful CI run attempt.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PackageReleaseSource {
    /// Full lowercase source commit SHA.
    pub commit_sha: String,
    /// Source ref, exactly `refs/heads/<default_branch>`.
    #[serde(rename = "ref")]
    pub source_ref: String,
    /// Exact configured default branch.
    pub default_branch: String,
    /// Source workflow event; currently only default-branch push.
    pub event: PackageReleaseEvent,
    /// Successful CI workflow run ID for this source commit.
    pub ci_run_id: u64,
    /// Exact successful CI run attempt.
    pub ci_run_attempt: u32,
}

/// Preview push or stable human-approved dispatch event.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PackageReleaseEvent {
    /// Push to the configured default branch.
    Push,
    /// Stable release explicitly dispatched from the configured default branch.
    WorkflowDispatch,
}

/// Workflow authority and run identity for build, signing, and publication.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PackageReleaseWorkflow {
    /// Tracked generated workflow path.
    pub path: String,
    /// Workflow ref, equal to the source default-branch ref.
    #[serde(rename = "ref")]
    pub workflow_ref: String,
    /// Full commit SHA supplying the workflow authority.
    pub authority_sha: String,
    /// Original publication workflow run ID and attempt, stable across recovery retries.
    /// A manual recovery invocation's run identity belongs in execution metadata,
    /// never in this canonical receipt.
    pub run_id: u64,
    /// Release workflow run attempt.
    pub run_attempt: u32,
    /// Producer workflow run ID and attempt retained for retry identity.
    pub producer_run_id: u64,
    /// Producer workflow run attempt.
    pub producer_run_attempt: u32,
    /// Stable generated signer job identity.
    pub signer_job_id: String,
    /// Workflow path executed under the signer identity.
    pub signer_workflow: String,
}

/// Deterministic package and channel version identity.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PackageReleaseVersion {
    /// Exact numeric Cargo version `X.Y.Z`.
    pub base: String,
    /// Stable base version or full preview version.
    pub value: String,
    /// Full-history count for preview; null for stable's shallow workflow.
    pub commit_count: Option<u64>,
    /// First seven lowercase hex characters of source SHA for preview; null for stable.
    pub source_sha7: Option<String>,
}

/// One Cargo package product and its target archive matrix.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PackageReleaseProduct {
    /// Stable product identity.
    pub id: String,
    /// Cargo package name.
    pub package: String,
    /// Exact package version, equal to `version.base`.
    pub package_version: String,
    /// Sorted executable members present in every archive.
    pub bins: Vec<String>,
    /// One sorted archive per declared target.
    pub payloads: Vec<PackageReleasePayload>,
}

/// Product archive and exact regular-file executable members.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PackageReleasePayload {
    /// Closed payload representation. Stable may publish its historical raw
    /// single-binary asset; preview packages use deterministic tarballs.
    pub format: PackageReleasePayloadFormat,
    /// Supported Rust target triple.
    pub target: String,
    /// Canonical release asset name.
    pub name: String,
    /// Sorted archive members, exactly equal to the product bins.
    pub members: Vec<String>,
}

/// Public artifact representation for one target-specific product payload.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PackageReleasePayloadFormat {
    /// One raw executable, retained for the existing stable binary interface.
    RawBinary,
    /// A gzip-compressed tar archive containing the declared executables.
    TarGz,
}

/// One explicitly declared non-binary package asset.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PackageReleaseCompanion {
    /// Stable companion identity.
    pub id: String,
    /// Canonical immutable release asset name.
    pub name: String,
}

/// Asset role inside the release inventory.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PackageReleaseAssetKind {
    /// Executable archive payload.
    Payload,
    /// SHA-256 sidecar for a payload.
    Sha256,
    /// Sigstore signature bundle for a payload.
    SignatureBundle,
    /// SBOM sidecar for a payload.
    Sbom,
    /// SHA-256 list for all payload archives.
    ChecksumList,
    /// Explicit companion payload.
    Companion,
    /// Sigstore signature bundle for a companion.
    CompanionSignatureBundle,
}

/// Immutable release asset identity and subject binding.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PackageReleaseAsset {
    /// Exact release asset filename.
    pub name: String,
    /// Closed asset role.
    pub kind: PackageReleaseAssetKind,
    /// Owning product for payloads and payload sidecars, otherwise null.
    pub product_id: Option<String>,
    /// Owning target for payloads and payload sidecars, otherwise null.
    pub target: Option<String>,
    /// Payload/companion whose signature or SBOM is bound; null for checksum list.
    pub subject_name: Option<String>,
    /// Exact byte length of the immutable asset.
    pub size: u64,
    /// SHA-256 of the immutable asset bytes.
    pub sha256: String,
}

/// Closed SBOM and workflow-attestation evidence declaration.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PackageReleaseEvidence {
    /// Mandatory `signed-sbom-v1` profile.
    pub profile: String,
    /// Exact supported SBOM serialization.
    pub sbom_format: PackageReleaseSbomFormat,
    /// OIDC issuer used for the exact signer identity.
    pub signer_issuer: String,
    /// Full source repository/workflow/ref signer identity.
    pub signer_workflow_ref: String,
    /// Exact attestation predicate type.
    pub attestation_predicate: String,
    /// Sorted payload, companion, and receipt attestation subjects.
    pub attested_subject_names: Vec<String>,
}

/// Supported SBOM format.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PackageReleaseSbomFormat {
    /// `CycloneDX-JSON`.
    #[serde(rename = "CycloneDX-JSON")]
    CycloneDxJson,
}

impl PackageReleaseReceipt {
    /// Validate the complete receipt contract.
    ///
    /// # Errors
    ///
    /// Returns an identity error when fields, ordering, identities, or asset
    /// associations violate the immutable artifact-set contract.
    pub fn validate(&self) -> Result<(), ContractError> {
        validation::validate(self)
    }

    /// Validate receipt inventories against the enabled preview configuration.
    ///
    /// # Errors
    ///
    /// Returns an identity error when product, target, companion, or asset
    /// inventory differs from policy.
    pub fn validate_against_preview(
        &self,
        preview: &crate::config::RustBinaryPreviewConfig,
    ) -> Result<(), ContractError> {
        validation::validate_against_preview(self, preview)
    }

    /// Validate a preview receipt against the static signed release plan.
    ///
    /// This form is suitable for isolated privileged consumers because it
    /// carries only literal product and companion inventory, never source task
    /// configuration or executable task declarations.
    ///
    /// # Errors
    /// Returns an identity error when receipt identity or inventory differs
    /// from the validated preview plan.
    pub fn validate_against_plan(
        &self,
        plan: &crate::package_release_plan::PackageReleasePlan,
    ) -> Result<(), ContractError> {
        validation::validate_against_plan(self, plan)
    }

    /// Serialize the typed receipt as deterministic canonical JSON bytes.
    ///
    /// # Errors
    ///
    /// Returns an error if canonical serialization fails.
    pub fn canonical_bytes(&self) -> Result<Vec<u8>, ContractError> {
        canonical_json_bytes(self)
    }

    /// BLAKE3 digest of canonical receipt bytes for retry identity.
    ///
    /// # Errors
    ///
    /// Returns an error if canonical serialization fails.
    pub fn canonical_digest(&self) -> Result<String, ContractError> {
        Ok(crate::canonical::digest_b3(&self.canonical_bytes()?))
    }

    /// Asset byte identities, already sorted by canonical asset name.
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

/// Byte identity used by publication retries and existing-release checks.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PackageReleaseAssetIdentity {
    /// Exact immutable asset filename.
    pub name: String,
    /// Exact byte length.
    pub size: u64,
    /// SHA-256 of exact asset bytes.
    pub sha256: String,
}

/// Parse, validate, and require byte-for-byte canonical v2 JSON.
///
/// # Errors
///
/// Rejects oversized, invalid UTF-8, duplicate-key, unknown-field, malformed,
/// noncanonical, or semantically invalid receipts.
pub use parse::parse_package_release_receipt;
