//! Closed `[[images]]` declarations for container image build and publication.
//!
//! Pure data plus lexical validation. Dockerfile content, filesystem state
//! and registry ownership are checked by the orchestrator and by the
//! generated jobs; this module accepts no shell, argv, YAML or `uses:` fields.

use super::workflow_task::is_valid_workflow_task_id;
use crate::errors::ContractError;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

/// Registry host every image repository must live under.
pub const IMAGE_REGISTRY_HOST: &str = "ghcr.io";
/// Longest accepted image id; job ids add `image-` and a platform suffix.
const MAX_IMAGE_ID_BYTES: usize = 40;

/// Closed native image platform catalog; one hosted runner per platform.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ImagePlatform {
    /// `linux/amd64` built on a hosted amd64 runner.
    LinuxAmd64,
    /// `linux/arm64` built on a hosted arm64 runner.
    LinuxArm64,
}

impl ImagePlatform {
    /// Config spelling, also the job-id suffix.
    #[must_use]
    pub const fn key(self) -> &'static str {
        match self {
            Self::LinuxAmd64 => "linux_amd64",
            Self::LinuxArm64 => "linux_arm64",
        }
    }

    /// Job-id-safe suffix.
    #[must_use]
    pub const fn job_suffix(self) -> &'static str {
        match self {
            Self::LinuxAmd64 => "amd64",
            Self::LinuxArm64 => "arm64",
        }
    }

    /// OCI platform string passed to Buildx.
    #[must_use]
    pub const fn oci_platform(self) -> &'static str {
        match self {
            Self::LinuxAmd64 => "linux/amd64",
            Self::LinuxArm64 => "linux/arm64",
        }
    }

    /// Exact hosted runner label that builds this platform natively.
    #[must_use]
    pub const fn runs_on(self) -> &'static str {
        match self {
            Self::LinuxAmd64 => "ubuntu-26.04",
            Self::LinuxArm64 => "ubuntu-26.04-arm",
        }
    }
}

/// Dockerfile location: one file, or one file per platform.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ImageDockerfile {
    /// One Dockerfile shared by every platform.
    Single(String),
    /// One Dockerfile per platform; keys must equal `platforms` exactly.
    PerPlatform(BTreeMap<ImagePlatform, String>),
}

impl ImageDockerfile {
    /// Dockerfile path used for `platform`, when declared.
    #[must_use]
    pub fn for_platform(&self, platform: ImagePlatform) -> Option<&str> {
        match self {
            Self::Single(path) => Some(path),
            Self::PerPlatform(map) => map.get(&platform).map(String::as_str),
        }
    }

    /// Every declared Dockerfile path, sorted and unique.
    #[must_use]
    pub fn paths(&self) -> BTreeSet<&str> {
        match self {
            Self::Single(path) => BTreeSet::from([path.as_str()]),
            Self::PerPlatform(map) => map.values().map(String::as_str).collect(),
        }
    }
}

/// Artifacts Velnor builds outside `BuildKit` and stages into the context.
///
/// Closed tagged union. `gradle_task` is added by the Gradle stack work.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ImagePrebuilt {
    /// A release Cargo binary built with `--locked --offline` through MBX.
    CargoBin {
        /// Workspace package name.
        package: String,
        /// Binary target name.
        bin: String,
        /// Path inside the build context the Dockerfile `COPY`s from.
        stage: String,
    },
}

/// One `[[images]]` entry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ImageConfig {
    /// Stable id, unique across images; names every generated job.
    pub id: String,
    /// `ghcr.io/<repo-owner-lowercase>/<name>`; the owner is bound at run time.
    pub repository: String,
    /// Dockerfile path, or one per platform.
    pub dockerfile: ImageDockerfile,
    /// Build context directory (repo-relative; `.` is the repository root).
    #[serde(default = "default_context")]
    pub context: String,
    /// Native platforms, sorted and unique; no emulation.
    pub platforms: Vec<ImagePlatform>,
    /// Manifest whose `version` names the promoted release tag.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version_file: Option<String>,
    /// Optional artifacts Velnor builds before `BuildKit`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prebuilt: Option<ImagePrebuilt>,
    /// Literal `timeout-minutes` of every build job of this image.
    #[serde(default = "default_timeout_minutes")]
    pub timeout_minutes: u16,
}

/// Default build-job bound; image builds outlast the 5-minute crate default.
pub const DEFAULT_IMAGE_TIMEOUT_MINUTES: u16 = 30;

fn default_timeout_minutes() -> u16 {
    DEFAULT_IMAGE_TIMEOUT_MINUTES
}

/// Generated build and publish job ids all start with this prefix.
pub const IMAGE_JOB_PREFIX: &str = "image-";

fn default_context() -> String {
    ".".to_owned()
}

impl ImageConfig {
    /// Generated build job id for `platform`.
    #[must_use]
    pub fn build_job_id(&self, platform: ImagePlatform) -> String {
        format!("image-{}-{}", self.id, platform.job_suffix())
    }

    /// Generated publish job id.
    #[must_use]
    pub fn publish_job_id(&self) -> String {
        format!("image-publish-{}", self.id)
    }

    /// Validate one entry; `index` locates it in error key paths.
    /// # Errors
    pub fn validate(&self, file: &str, index: usize) -> Result<(), ContractError> {
        let at = |field: &str| format!("images[{index}].{field}");
        if !is_valid_image_id(&self.id) {
            return Err(ContractError::config(
                file,
                at("id"),
                format!("bad_image_id:{}", self.id),
            ));
        }
        if !is_valid_image_repository(&self.repository) {
            return Err(ContractError::config(
                file,
                at("repository"),
                format!("bad_image_repository:{}", self.repository),
            ));
        }
        if !crate::workflow::timeout::JobTimeout::is_valid_minutes(u32::from(self.timeout_minutes))
        {
            return Err(ContractError::config(
                file,
                at("timeout_minutes"),
                format!("bad_image_timeout:{}", self.timeout_minutes),
            ));
        }
        self.validate_platforms(file, &at("platforms"))?;
        self.validate_paths(file, &at)?;
        if let Some(prebuilt) = &self.prebuilt {
            prebuilt.validate(file, &at("prebuilt"), &self.context)?;
        }
        Ok(())
    }

    fn validate_platforms(&self, file: &str, key: &str) -> Result<(), ContractError> {
        if self.platforms.is_empty() {
            return Err(ContractError::config(file, key, "empty_image_platforms"));
        }
        if self.platforms.windows(2).any(|pair| pair[0] >= pair[1]) {
            return Err(ContractError::config(
                file,
                key,
                "image_platforms_not_sorted_unique",
            ));
        }
        if let ImageDockerfile::PerPlatform(map) = &self.dockerfile {
            let declared = self.platforms.iter().copied().collect::<BTreeSet<_>>();
            let mapped = map.keys().copied().collect::<BTreeSet<_>>();
            if declared != mapped {
                return Err(ContractError::config(
                    file,
                    key,
                    "image_dockerfile_platforms_mismatch",
                ));
            }
        }
        Ok(())
    }

    fn validate_paths(&self, file: &str, at: &dyn Fn(&str) -> String) -> Result<(), ContractError> {
        for path in self.dockerfile.paths() {
            require_safe_path(file, at("dockerfile"), path, false)?;
        }
        require_safe_path(file, at("context"), &self.context, true)?;
        if let Some(version_file) = &self.version_file {
            require_safe_path(file, at("version_file"), version_file, false)?;
        }
        Ok(())
    }
}

impl ImagePrebuilt {
    fn validate(&self, file: &str, key: &str, context: &str) -> Result<(), ContractError> {
        match self {
            Self::CargoBin {
                package,
                bin,
                stage,
            } => {
                for (field, value) in [("package", package), ("bin", bin)] {
                    if !is_cargo_name(value) {
                        return Err(ContractError::config(
                            file,
                            format!("{key}.{field}"),
                            format!("bad_image_cargo_name:{value}"),
                        ));
                    }
                }
                require_safe_path(file, format!("{key}.stage"), stage, false)?;
                if context != "." && !stage.starts_with(&format!("{context}/")) {
                    return Err(ContractError::config(
                        file,
                        format!("{key}.stage"),
                        "image_stage_outside_context",
                    ));
                }
                Ok(())
            }
        }
    }
}

/// Validate the whole `[[images]]` inventory: entries, then uniqueness.
/// # Errors
pub fn validate_images(images: &[ImageConfig], file: &str) -> Result<(), ContractError> {
    let mut ids = BTreeSet::new();
    let mut repositories = BTreeSet::new();
    for (index, image) in images.iter().enumerate() {
        image.validate(file, index)?;
        if !ids.insert(image.id.as_str()) {
            return Err(ContractError::config(
                file,
                format!("images[{index}].id"),
                "duplicate_image_id",
            ));
        }
        if !repositories.insert(image.repository.as_str()) {
            return Err(ContractError::config(
                file,
                format!("images[{index}].repository"),
                "duplicate_image_repository",
            ));
        }
    }
    Ok(())
}

/// Image id: workflow-task grammar, bounded so job ids stay short.
#[must_use]
pub fn is_valid_image_id(id: &str) -> bool {
    id.len() <= MAX_IMAGE_ID_BYTES && is_valid_workflow_task_id(id)
}

/// `ghcr.io/<owner>/<name>[/<name>...]`, lowercase, no tag or digest.
#[must_use]
pub fn is_valid_image_repository(repository: &str) -> bool {
    let Some(rest) = repository
        .strip_prefix(IMAGE_REGISTRY_HOST)
        .and_then(|rest| rest.strip_prefix('/'))
    else {
        return false;
    };
    let segments = rest.split('/').collect::<Vec<_>>();
    segments.len() >= 2
        && segments.iter().enumerate().all(|(position, segment)| {
            if position == 0 {
                is_owner_segment(segment)
            } else {
                is_name_segment(segment)
            }
        })
}

fn is_owner_segment(segment: &str) -> bool {
    !segment.is_empty()
        && !segment.starts_with('-')
        && !segment.ends_with('-')
        && segment
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

fn is_name_segment(segment: &str) -> bool {
    let bytes = segment.as_bytes();
    !bytes.is_empty()
        && bytes[0].is_ascii_alphanumeric()
        && bytes[bytes.len() - 1].is_ascii_alphanumeric()
        && !segment.contains("..")
        && bytes.iter().all(|b| {
            b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'-' | b'_' | b'.')
        })
}

fn is_cargo_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_'))
        && !name.starts_with('-')
}

/// Lexical repo-relative path check shared by every image path field.
fn require_safe_path(
    file: &str,
    key: String,
    path: &str,
    allow_dot: bool,
) -> Result<(), ContractError> {
    if allow_dot && path == "." {
        return Ok(());
    }
    let bad = path.is_empty()
        || path.len() > 256
        || path.starts_with('/')
        || path.ends_with('/')
        || path.split('/').any(|c| matches!(c, "" | "." | ".."))
        || !path.bytes().all(|b| {
            b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'/' | b'+' | b'@')
        });
    if bad {
        return Err(ContractError::config(
            file,
            key,
            format!("bad_image_path:{path}"),
        ));
    }
    Ok(())
}
