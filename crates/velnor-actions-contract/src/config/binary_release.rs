// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::workflow::JobTimeout;
use serde::{Deserialize, Serialize};

#[path = "binary_release_preview.rs"]
mod preview;
#[path = "binary_release_publication_budget.rs"]
mod publication_budget;
#[path = "binary_release_validation.rs"]
mod validation;
pub use preview::*;
pub use publication_budget::{
    MAX_PUBLICATION_RUNTIME_MINUTES, PUBLICATION_CLEANUP_HEADROOM_MINUTES,
    PUBLICATION_PREPARATION_STEP_DEFAULT_MINUTES, RELEASE_ACQUIRE_STEP_TIMEOUT_MINUTES,
};
pub(crate) use validation::validate_static_release_inventory;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RustBinaryReleaseConfig {
    #[serde(default = "default_manifest_path")]
    pub manifest_path: String,

    #[serde(default)]
    pub version_package: String,

    #[serde(default)]
    pub version_env: String,

    #[serde(default)]
    pub producer: RustBinaryReleaseProducer,

    #[serde(default = "default_publication_timeout")]
    pub publication_timeout_minutes: JobTimeout,

    #[serde(default)]
    pub products: Vec<RustBinaryReleaseProduct>,

    #[serde(default)]
    pub companions: Vec<RustBinaryReleaseCompanion>,

    pub evidence: RustBinaryReleaseEvidence,

    #[serde(default)]
    pub preview: RustBinaryPreviewConfig,

    #[serde(default)]
    pub stable: RustBinaryStableConfig,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RustBinaryStableConfig {
    #[serde(default)]
    pub enabled: bool,

    #[serde(default = "default_stable_environment")]
    pub environment: String,
}

fn default_manifest_path() -> String {
    "Cargo.toml".to_owned()
}
fn default_publication_timeout() -> JobTimeout {
    JobTimeout::DEFAULT
}
fn default_stable_environment() -> String {
    "release".to_owned()
}

impl Default for RustBinaryStableConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            environment: default_stable_environment(),
        }
    }
}

impl Default for RustBinaryReleaseConfig {
    fn default() -> Self {
        Self {
            manifest_path: default_manifest_path(),
            version_package: String::new(),
            version_env: String::new(),
            producer: RustBinaryReleaseProducer::default(),
            publication_timeout_minutes: default_publication_timeout(),
            products: Vec::new(),
            companions: Vec::new(),
            evidence: RustBinaryReleaseEvidence::default(),
            preview: RustBinaryPreviewConfig::default(),
            stable: RustBinaryStableConfig::default(),
        }
    }
}

impl RustBinaryReleaseConfig {
    #[must_use]
    pub const fn enabled(&self) -> bool {
        self.preview.enabled || self.stable.enabled
    }

    #[must_use]
    pub const fn channel_enabled(
        &self,
        channel: crate::package_release::PackageReleaseChannel,
    ) -> bool {
        match channel {
            crate::package_release::PackageReleaseChannel::Preview => self.preview.enabled,
            crate::package_release::PackageReleaseChannel::Stable => self.stable.enabled,
        }
    }
}
