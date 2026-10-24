//! Shared native release inventory with closed rolling and stable policies.

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

/// Shared inventory and qualified producer for both native release channels.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RustBinaryReleaseConfig {
    /// Repo-relative Cargo workspace manifest.
    #[serde(default = "default_manifest_path")]
    pub manifest_path: String,
    /// Discovered Cargo package owning the shared base version.
    #[serde(default)]
    pub version_package: String,
    /// Application version supplied only by the native channel authority.
    #[serde(default)]
    pub version_env: String,
    /// Qualified credential-free producer shared by both channels.
    #[serde(default)]
    pub producer: RustBinaryReleaseProducer,
    /// Configured native publication deadline; defaults independently of producer budgets.
    /// Publisher and recovery jobs add preparation-step caps and cleanup headroom.
    /// Preview source eligibility continues to use this value as its outer job limit.
    #[serde(default = "default_publication_timeout")]
    pub publication_timeout_minutes: JobTimeout,
    /// Closed explicit product matrix.
    #[serde(default)]
    pub products: Vec<RustBinaryReleaseProduct>,
    /// Closed non-binary asset inventory.
    #[serde(default)]
    pub companions: Vec<RustBinaryReleaseCompanion>,
    /// Complete mandatory evidence profile.
    pub evidence: RustBinaryReleaseEvidence,
    /// Automatic rolling preview policy.
    #[serde(default)]
    pub preview: RustBinaryPreviewConfig,
    /// Manual permanent stable release policy.
    #[serde(default)]
    pub stable: RustBinaryStableConfig,
}

/// Manual stable publication; there is no automatic source-push variant.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RustBinaryStableConfig {
    /// Whether manual stable release preparation/publication is generated.
    #[serde(default)]
    pub enabled: bool,
    /// Protected stable publication environment.
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
    /// Whether either native channel is enabled.
    #[must_use]
    pub const fn enabled(&self) -> bool {
        self.preview.enabled || self.stable.enabled
    }

    /// Select policy from the authenticated channel, never a release URL.
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

#[cfg(test)]
#[path = "binary_release_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "binary_release_channels_tests.rs"]
mod channel_tests;
