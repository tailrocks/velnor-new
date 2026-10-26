// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::image_grammar::{
    is_cargo_name, is_valid_image_id, is_valid_image_repository, require_safe_path,
};
use crate::errors::ContractError;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

pub const IMAGE_REGISTRY_HOST: &str = "ghcr.io";

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ImagePlatform {
    LinuxAmd64,

    LinuxArm64,
}

impl ImagePlatform {
    #[must_use]
    pub const fn key(self) -> &'static str {
        match self {
            Self::LinuxAmd64 => "linux_amd64",
            Self::LinuxArm64 => "linux_arm64",
        }
    }

    #[must_use]
    pub const fn job_suffix(self) -> &'static str {
        match self {
            Self::LinuxAmd64 => "amd64",
            Self::LinuxArm64 => "arm64",
        }
    }

    #[must_use]
    pub const fn oci_platform(self) -> &'static str {
        match self {
            Self::LinuxAmd64 => "linux/amd64",
            Self::LinuxArm64 => "linux/arm64",
        }
    }

    #[must_use]
    pub const fn runs_on(self) -> &'static str {
        match self {
            Self::LinuxAmd64 => "ubuntu-26.04",
            Self::LinuxArm64 => "ubuntu-26.04-arm",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ImageDockerfile {
    Single(String),

    PerPlatform(BTreeMap<ImagePlatform, String>),
}

impl ImageDockerfile {
    #[must_use]
    pub fn for_platform(&self, platform: ImagePlatform) -> Option<&str> {
        match self {
            Self::Single(path) => Some(path),
            Self::PerPlatform(map) => map.get(&platform).map(String::as_str),
        }
    }

    #[must_use]
    pub fn paths(&self) -> BTreeSet<&str> {
        match self {
            Self::Single(path) => BTreeSet::from([path.as_str()]),
            Self::PerPlatform(map) => map.values().map(String::as_str).collect(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ImagePrebuilt {
    CargoBin {
        package: String,

        bin: String,

        stage: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ImageConfig {
    pub id: String,

    pub repository: String,

    pub dockerfile: ImageDockerfile,

    #[serde(default = "default_context")]
    pub context: String,

    pub platforms: Vec<ImagePlatform>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version_file: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prebuilt: Option<ImagePrebuilt>,

    #[serde(default = "default_timeout_minutes")]
    pub timeout_minutes: u16,
}

pub const DEFAULT_IMAGE_TIMEOUT_MINUTES: u16 = 30;

fn default_timeout_minutes() -> u16 {
    DEFAULT_IMAGE_TIMEOUT_MINUTES
}

pub const IMAGE_JOB_PREFIX: &str = "image-";

fn default_context() -> String {
    ".".to_owned()
}

impl ImageConfig {
    #[must_use]
    pub fn build_job_id(&self, platform: ImagePlatform) -> String {
        format!("image-{}-{}", self.id, platform.job_suffix())
    }

    #[must_use]
    pub fn publish_job_id(&self) -> String {
        format!("image-publish-{}", self.id)
    }

    #[must_use]
    pub fn promote_job_id(&self) -> String {
        format!("image-promote-{}", self.id)
    }

    fn generated_job_ids(&self) -> Vec<String> {
        let mut ids: Vec<String> = self
            .platforms
            .iter()
            .map(|platform| self.build_job_id(*platform))
            .collect();
        ids.push(self.publish_job_id());
        ids.push(self.promote_job_id());
        ids
    }

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
            if super::publish::VersionSource::for_path(version_file).is_none() {
                return Err(ContractError::config(
                    file,
                    at("version_file"),
                    format!("unsupported_image_version_file:{version_file}"),
                ));
            }
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

pub fn validate_images(images: &[ImageConfig], file: &str) -> Result<(), ContractError> {
    let mut ids = BTreeSet::new();
    let mut repositories = BTreeSet::new();
    let mut job_ids = BTreeSet::new();
    for (index, image) in images.iter().enumerate() {
        image.validate(file, index)?;
        for job_id in image.generated_job_ids() {
            if !job_ids.insert(job_id.clone()) {
                return Err(ContractError::config(
                    file,
                    format!("images[{index}].id"),
                    format!("image_job_id_collision:{job_id}"),
                ));
            }
        }
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
