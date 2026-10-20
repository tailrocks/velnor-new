//! Typed GitHub Actions step output identities.

use serde::{Deserialize, Serialize};

/// Stable GitHub Actions id for a step whose outputs or outcome have consumers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum StepId {
    /// Planner output consumed by downstream jobs.
    Plan,
    /// Baseline publisher output consumed by the artifact uploader.
    PublishBaseline,
    /// V2 hosted tools-cache output consumed by restore and save.
    ToolsCacheIdentity,
    /// Staging path output for the normal tools-cache payload.
    NormalToolsCacheStage,
    /// Import result and admission outputs for the normal tools-cache payload.
    NormalToolsCacheImport,
    /// Staged payload identity outputs for the normal tools-cache save.
    NormalToolsCacheExport,
    /// Action-local PATH containing verified cache tar aliases.
    CacheTarLaunchers,
    /// Verified staged helper digest used by the tar adapter installer.
    PreseedStage,
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
}

impl StepId {
    /// Fixed YAML spelling for this step output owner.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Plan => "plan",
            Self::PublishBaseline => "publish-baseline",
            Self::ToolsCacheIdentity => "v2",
            Self::NormalToolsCacheStage => "normal-tools-cache-stage",
            Self::NormalToolsCacheImport => "normal-tools-cache-import",
            Self::NormalToolsCacheExport => "normal-tools-cache-export",
            Self::CacheTarLaunchers => "cache-tar-launchers",
            Self::PreseedStage => "preseed-stage",
            Self::TofuProviders => "tofu-providers",
            Self::MbxReady => "mbx-ready",
            Self::CacheKeys => "cache-keys",
            Self::SourcesRestore => "sources-restore",
            Self::SourcesFetch => "sources-fetch",
            Self::ToolsRestore => "tools-restore",
            Self::DepsToken => "deps-token",
        }
    }
}
