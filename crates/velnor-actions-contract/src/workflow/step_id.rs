//! Typed GitHub Actions step output identities.

use serde::{Deserialize, Serialize};

use super::qualification_cache_lineage::QualificationCacheLayer;

/// Step ID for one job, cache layer, and action stage.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QualificationCacheStepStage {
    /// Connect one job ID and cache layer to the checked directive.
    Bind,
    /// Restore the exact archive for this job and cache layer.
    Restore,
    /// Save the exact archive for this job and cache layer.
    Save,
    /// Read the runtime identity of one job's cache layer.
    RuntimeIdentity,
    /// Read the exact roots for one job's cache layer.
    Roots,
}

/// Step ID for one job, cache layer, and action stage.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QualificationCacheStepId {
    stage: QualificationCacheStepStage,
    layer: QualificationCacheLayer,
    owner_tag: u64,
}

impl QualificationCacheStepId {
    /// Build an output ID from a checked job ID, cache layer, and action stage.
    ///
    /// # Errors
    ///
    /// Returns an error when the job ID does not use the required format.
    pub fn new(
        stage: QualificationCacheStepStage,
        layer: QualificationCacheLayer,
        owner_id: &str,
    ) -> Result<Self, crate::errors::ContractError> {
        crate::workflow::validate_job_id(owner_id)?;
        let digest = blake3::hash(owner_id.as_bytes());
        let mut tag = [0_u8; 8];
        tag.copy_from_slice(&digest.as_bytes()[..8]);
        Ok(Self {
            stage,
            layer,
            owner_tag: u64::from_be_bytes(tag),
        })
    }

    /// Return the GitHub Actions step ID.
    #[must_use]
    pub fn as_string(self) -> String {
        format!(
            "qualification-cache-{}-{}-{:016x}",
            self.stage.as_str(),
            self.layer.as_str(),
            self.owner_tag
        )
    }

    /// Action stage named by this ID.
    #[must_use]
    pub const fn stage(self) -> QualificationCacheStepStage {
        self.stage
    }

    /// Check whether this ID was made for the given job ID.
    #[must_use]
    pub fn belongs_to(self, owner_id: &str) -> bool {
        let digest = blake3::hash(owner_id.as_bytes());
        let mut tag = [0_u8; 8];
        tag.copy_from_slice(&digest.as_bytes()[..8]);
        self.owner_tag == u64::from_be_bytes(tag)
    }
}

impl QualificationCacheStepStage {
    /// Fixed text for this action stage.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Bind => "bind",
            Self::Restore => "restore",
            Self::Save => "save",
            Self::RuntimeIdentity => "runtime-identity",
            Self::Roots => "roots",
        }
    }
}

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
    /// Qualification cache action ID for one lane and layer.
    QualificationCache(QualificationCacheStepId),
    /// File with runtime identity data used to bind the qualification cache.
    QualificationRuntimeIdentity,
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
            Self::QualificationCache(_) => "qualification-cache",
            Self::QualificationRuntimeIdentity => "qualification-runtime-identity",
        }
    }

    /// Return the full YAML ID, including the job and cache layer.
    #[must_use]
    pub fn as_string(self) -> String {
        match self {
            Self::QualificationCache(identity) => identity.as_string(),
            fixed => fixed.as_str().to_owned(),
        }
    }
}
