//! Typed GitHub Actions step output identities.

use serde::{Deserialize, Serialize};

/// Internal operation of the Gradle run step (verify wrapper, install JDK, run Gradle).
pub const GRADLE_RUN_OPERATION: &str = "gradle-run-v1";

mod qualification_cache;
pub use qualification_cache::{
    QualificationCacheCompositeId, QualificationCacheCompositePhase, QualificationCacheStepId,
    QualificationCacheStepStage, qualification_cache_after_save_input_names,
    qualification_cache_after_save_output_names, qualification_cache_before_save_output_names,
    qualification_cache_composite_output_names, qualification_cache_layer_prepare_input_names,
};
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
    /// Restore and save steps use this MBX bundle key output.
    MbxBundleKey,
    /// Import and save steps use this MBX bundle restore output.
    MbxBundleRestore,
    /// The cache save step uses this MBX bundle export output.
    MbxBundleExport,
    /// Lane/layer owner for a qualification cache action.
    QualificationCache(QualificationCacheStepId),
    /// Typed output owner for one grouped qualification-cache composite.
    QualificationCacheComposite(QualificationCacheCompositeId),
    /// Runtime identity file consumed by qualification cache binding.
    QualificationRuntimeIdentity,
    /// Qualification receipt file consumed by the Required artifact upload.
    QualificationCacheReceipt,
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
            Self::MbxBundleKey => "mbx-bundle-key",
            Self::MbxBundleRestore => "mbx-bundle-restore",
            Self::MbxBundleExport => "mbx-bundle-export",
            Self::QualificationCache(_) => "qualification-cache",
            Self::QualificationCacheComposite(_) => "qualification-cache-composite",
            Self::QualificationRuntimeIdentity => "qualification-runtime-identity",
            Self::QualificationCacheReceipt => "qualification-cache-receipt",
        }
    }

    /// Render the complete YAML ID, including a typed owner/layer identity.
    #[must_use]
    pub fn as_string(self) -> String {
        match self {
            Self::QualificationCache(identity) => identity.as_string(),
            Self::QualificationCacheComposite(identity) => identity.as_string(),
            fixed => fixed.as_str().to_owned(),
        }
    }
}
