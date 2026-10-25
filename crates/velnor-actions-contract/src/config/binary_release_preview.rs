use super::RustBinaryReleaseConfig;
use crate::config::{CheckSystemTool, MiseTaskSource, VerificationRunner};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RustBinaryPreviewConfig {
    #[serde(default)]
    pub enabled: bool,

    #[serde(default)]
    pub versioning: RustBinaryPreviewVersioning,

    #[serde(default = "default_preview_environment")]
    pub environment: String,
}

impl Default for RustBinaryPreviewConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            versioning: RustBinaryPreviewVersioning::PackagePreviewCommitCountSha7,
            environment: default_preview_environment(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RustBinaryPreviewVersioning {
    #[default]
    PackagePreviewCommitCountSha7,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RustBinaryReleaseEvidence {
    pub profile: RustBinaryReleaseEvidenceProfile,
}

impl Default for RustBinaryReleaseEvidence {
    fn default() -> Self {
        Self {
            profile: RustBinaryReleaseEvidenceProfile::SignedSbomV1,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RustBinaryReleaseEvidenceProfile {
    #[default]
    SignedSbomV1,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RustBinaryReleaseProducer {
    #[serde(default)]
    pub task: String,

    #[serde(default)]
    pub source: MiseTaskSource,

    #[serde(default = "default_preview_output_directory")]
    pub output_directory: String,

    #[serde(default = "default_preview_manifest")]
    pub manifest: String,

    #[serde(default)]
    pub tools: Vec<String>,

    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub system_tools: Vec<CheckSystemTool>,

    #[serde(default)]
    pub inputs: Vec<String>,

    #[serde(default = "default_preview_runner")]
    pub runner: VerificationRunner,

    #[serde(default = "default_preview_timeout")]
    pub timeout_minutes: u16,

    #[serde(default = "default_build_jobs")]
    pub build_jobs: u16,
}

impl RustBinaryReleaseProducer {
    pub const MAX_TIMEOUT_MINUTES: u16 = 30;

    pub const JOB_HEADROOM_MINUTES: u16 = 5;
}

impl Default for RustBinaryReleaseProducer {
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RustBinaryReleaseProduct {
    pub id: String,

    pub package: String,

    pub bins: Vec<String>,

    pub targets: Vec<String>,

    pub archive: RustBinaryReleaseArchive,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RustBinaryReleaseArchive {
    #[serde(rename = "tar.gz")]
    TarGz,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RustBinaryReleaseCompanion {
    pub id: String,

    pub path: String,

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
impl RustBinaryReleaseConfig {
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
