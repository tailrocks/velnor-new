//! Closed configuration contract for Rust binary preview publication.

use crate::config::{CheckSystemTool, MiseTaskSource, VerificationRunner};
use serde::{Deserialize, Serialize};

#[cfg(test)]
#[path = "binary_release_preview_tests.rs"]
mod tests;
#[path = "binary_release_preview_validation.rs"]
mod validation;
pub(crate) use validation::validate_static_release_inventory;

/// Independently enabled Rust binary preview policy.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RustBinaryPreviewConfig {
    /// Whether to emit native preview publication.
    #[serde(default)]
    pub enabled: bool,
    /// Fixed native preview version derivation algorithm.
    #[serde(default)]
    pub versioning: RustBinaryPreviewVersioning,
    /// Application build variable populated only by Velnor.
    #[serde(default)]
    pub version_env: String,
    /// Protected environment authorizing automatic preview publication.
    #[serde(default = "default_preview_environment")]
    pub environment: String,
    /// Isolated, repository-owned product builder.
    #[serde(default)]
    pub producer: RustBinaryPreviewProducer,
    /// Explicit Cargo product inventory.
    #[serde(default)]
    pub products: Vec<RustBinaryPreviewProduct>,
    /// Explicit non-binary release assets produced by the producer.
    #[serde(default)]
    pub companions: Vec<RustBinaryPreviewCompanion>,
    /// Closed mandatory proof profile. No individual evidence switches.
    pub evidence: RustBinaryPreviewEvidence,
}

impl RustBinaryPreviewConfig {
    /// Derive every release asset name from the typed product and companion inventory.
    ///
    /// Includes publisher-owned digests, signatures, SBOMs, checksum list,
    /// and canonical receipt names. The producer manifest is not published.
    #[must_use]
    pub fn expected_asset_names(&self) -> Vec<String> {
        let mut names = Vec::new();
        for product in &self.products {
            for target in &product.targets {
                let archive = format!("{}-{target}.tar.gz", product.id);
                names.push(archive.clone());
                names.push(format!("{archive}.sha256"));
                names.push(format!("{archive}.bundle"));
                names.push(format!("{archive}.sbom.json"));
            }
        }
        for companion in &self.companions {
            names.push(companion.asset_name.clone());
            names.push(format!("{}.bundle", companion.asset_name));
        }
        names.extend([
            "SHA256SUMS".to_owned(),
            "release.json".to_owned(),
            "release.json.bundle".to_owned(),
        ]);
        names
    }
}

impl Default for RustBinaryPreviewConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            versioning: RustBinaryPreviewVersioning::PackagePreviewCommitCountSha7,
            version_env: String::new(),
            environment: default_preview_environment(),
            producer: RustBinaryPreviewProducer::default(),
            products: Vec::new(),
            companions: Vec::new(),
            evidence: RustBinaryPreviewEvidence::default(),
        }
    }
}

/// Version algorithm. User supplied templates or expressions are forbidden.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RustBinaryPreviewVersioning {
    /// `<cargo-version>-preview.<all-history-commit-count>+<source-sha7>`.
    #[default]
    PackagePreviewCommitCountSha7,
}

/// Mandatory immutable release evidence profile.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RustBinaryPreviewEvidence {
    /// Complete required identity and integrity proof set.
    pub profile: RustBinaryPreviewEvidenceProfile,
}

impl Default for RustBinaryPreviewEvidence {
    fn default() -> Self {
        Self {
            profile: RustBinaryPreviewEvidenceProfile::SignedSbomV1,
        }
    }
}

/// Fixed preview release evidence profile.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RustBinaryPreviewEvidenceProfile {
    /// Checksums, `CycloneDX` JSON SBOMs, Sigstore signatures, and GitHub attestations.
    #[default]
    SignedSbomV1,
}

/// Qualified repository-owned task producing the declared inventory.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RustBinaryPreviewProducer {
    /// Exact task name in the selected Mise source.
    #[serde(default)]
    pub task: String,
    /// Tracked Mise source and confined working directory.
    #[serde(default)]
    pub source: MiseTaskSource,
    /// Repository-relative output root, independently scanned by Velnor.
    #[serde(default = "default_preview_output_directory")]
    pub output_directory: String,
    /// Producer inventory manifest; treated as untrusted build output.
    #[serde(default = "default_preview_manifest")]
    pub manifest: String,
    /// Qualified tool IDs required by the task.
    #[serde(default)]
    pub tools: Vec<String>,
    /// Exact host-installed native tool pins (macOS only).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub system_tools: Vec<CheckSystemTool>,
    /// Complete declared repository input closure, relative to repository root.
    #[serde(default)]
    pub inputs: Vec<String>,
    /// Fixed supported runner placement.
    #[serde(default = "default_preview_runner")]
    pub runner: VerificationRunner,
    /// Per-job timeout, bounded to one through 360 minutes (default five).
    #[serde(default = "default_preview_timeout")]
    pub timeout_minutes: u16,
    /// Maximum concurrent build processes.
    #[serde(default = "default_build_jobs")]
    pub build_jobs: u16,
}

impl Default for RustBinaryPreviewProducer {
    fn default() -> Self {
        Self {
            task: String::new(),
            source: MiseTaskSource::default(),
            output_directory: default_preview_output_directory(),
            manifest: default_preview_manifest(),
            tools: Vec::new(),
            system_tools: Vec::new(),
            inputs: Vec::new(),
            runner: default_preview_runner(),
            timeout_minutes: default_preview_timeout(),
            build_jobs: default_build_jobs(),
        }
    }
}

/// One Cargo package and its explicit binary/target matrix.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RustBinaryPreviewProduct {
    /// Stable unique product identity.
    pub id: String,
    /// Exact Cargo package name.
    pub package: String,
    /// Exact binary targets built from this package.
    pub bins: Vec<String>,
    /// Exact Rust target triples built for each binary.
    pub targets: Vec<String>,
    /// Closed archive format used for each binary payload.
    pub archive: RustBinaryPreviewArchive,
}

/// Supported immutable preview archive format.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RustBinaryPreviewArchive {
    /// Gzip-compressed tar archive.
    #[serde(rename = "tar.gz")]
    TarGz,
}

/// One declared companion asset, such as a capsule manifest.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RustBinaryPreviewCompanion {
    /// Stable unique asset identity.
    pub id: String,
    /// Producer output path relative to the producer output directory.
    pub path: String,
    /// Exact immutable release asset filename.
    pub asset_name: String,
}

fn default_preview_environment() -> String {
    "preview-publish".to_owned()
}
fn default_preview_output_directory() -> String {
    "dist/preview".to_owned()
}
fn default_preview_manifest() -> String {
    "dist/preview/package-output.json".to_owned()
}
fn default_preview_runner() -> VerificationRunner {
    VerificationRunner::LinuxX64
}
const fn default_preview_timeout() -> u16 {
    crate::workflow::timeout::JobTimeout::DEFAULT.minutes()
}
const fn default_build_jobs() -> u16 {
    2
}
