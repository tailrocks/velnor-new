//! Typed GitHub Actions step output identities.

use serde::{Deserialize, Serialize};

/// Internal operation of the Gradle run step (verify wrapper, install JDK, run Gradle).
pub const GRADLE_RUN_OPERATION: &str = "gradle-run-v1";

/// Stable GitHub Actions id for a step whose outputs or outcome have consumers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum StepId {
    /// Planner output consumed by downstream jobs.
    Plan,
    /// V2 hosted tools-cache output consumed by restore and save.
    ToolsCacheIdentity,
    /// `OpenTofu` provider-cache composite outputs consumed by the save step.
    TofuProviders,
    /// Successful MBX version/store guard that enables final workspace cleanup.
    MbxReady,
    /// Runtime cache trust and family keys consumed by restores and saves.
    CacheKeys,
    /// Plan-job sources restore whose hit outputs gate the sources save.
    SourcesRestore,
    /// Last Plan-job fetch step whose outcome gates the sources save.
    SourcesFetch,
    /// V2 tools restore whose exact-hit output gates the tools save.
    ToolsRestore,
    /// Private-source App token mint whose output feeds the fetch and revoke steps.
    DepsToken,
    /// MBX bundle key step whose outputs feed the restore and save.
    MbxBundleKey,
    /// MBX bundle restore whose hit outputs gate the import and the save.
    MbxBundleRestore,
    /// MBX bundle export whose `exported` output gates the save.
    MbxBundleExport,
}

impl StepId {
    /// Fixed YAML spelling for this step output owner.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Plan => "plan",
            Self::ToolsCacheIdentity => "v2",
            Self::TofuProviders => "tofu-providers",
            Self::MbxReady => "mbx-ready",
            Self::CacheKeys => "cache-keys",
            Self::SourcesRestore => "sources-restore",
            Self::SourcesFetch => "sources-fetch",
            Self::ToolsRestore => "tools-restore",
            Self::DepsToken => "deps-token",
            Self::MbxBundleKey => "mbx-bundle-key",
            Self::MbxBundleRestore => "mbx-bundle-restore",
            Self::MbxBundleExport => "mbx-bundle-export",
        }
    }
}
