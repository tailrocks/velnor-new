//! Typed GitHub Actions step output identities.

use serde::{Deserialize, Serialize};

use super::super::qualification_cache_lineage::QualificationCacheLayer;

/// Closed owner for one job-and-layer qualification cache action output.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QualificationCacheStepStage {
    /// Bind one job owner and layer to the validated directive.
    Bind,
    /// Restore one job owner's exact layer archive.
    Restore,
    /// Save one job owner's exact layer archive.
    Save,
    /// Measure one job owner's layer runtime identity.
    RuntimeIdentity,
    /// Measure one job owner's exact layer roots.
    Roots,
    /// Re-measure runtime identity after a cache import.
    RuntimeIdentityAfterImport,
    /// Observe backend state for a restore.
    RestoreCensus,
    /// Import one validated cache payload.
    Import,
    /// Export one cache payload after the task group.
    Export,
    /// Observe backend state before a save.
    SaveBeforeCensus,
    /// Observe backend state after a save.
    SaveAfterCensus,
    /// Write the immutable evidence for one owner and layer.
    OwnerLayerEvidence,
    /// Aggregate all layer evidence for one job owner.
    OwnerAggregate,
}

/// Typed identity for a job-and-layer qualification cache action.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QualificationCacheStepId {
    stage: QualificationCacheStepStage,
    layer: QualificationCacheLayer,
    owner_tag: u64,
}

impl QualificationCacheStepId {
    /// Build an output identity from one validated job owner ID.
    ///
    /// # Errors
    ///
    /// Returns an error when the owner ID does not use the canonical format.
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

    /// Render the validated ID for a GitHub Actions step.
    #[must_use]
    pub fn as_string(self) -> String {
        format!(
            "qualification-cache-{}-{}-{:016x}",
            self.stage.as_str(),
            self.layer.as_str(),
            self.owner_tag
        )
    }

    /// Stage for this action output owner.
    #[must_use]
    pub const fn stage(self) -> QualificationCacheStepStage {
        self.stage
    }

    /// Cache layer for this action output owner.
    #[must_use]
    pub const fn layer(self) -> QualificationCacheLayer {
        self.layer
    }

    /// Preserve layer/owner identity while selecting another fixed stage.
    #[must_use]
    pub(crate) const fn for_stage(self, stage: QualificationCacheStepStage) -> Self {
        Self { stage, ..self }
    }

    /// Check whether this output identity belongs to the supplied job ID.
    #[must_use]
    pub fn belongs_to(self, owner_id: &str) -> bool {
        let digest = blake3::hash(owner_id.as_bytes());
        let mut tag = [0_u8; 8];
        tag.copy_from_slice(&digest.as_bytes()[..8]);
        self.owner_tag == u64::from_be_bytes(tag)
    }
}

impl QualificationCacheStepStage {
    /// Fixed ID spelling for this output stage.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Bind => "bind",
            Self::Restore => "restore",
            Self::Save => "save",
            Self::RuntimeIdentity => "runtime-identity",
            Self::Roots => "roots",
            Self::RuntimeIdentityAfterImport => "runtime-identity-after-import",
            Self::RestoreCensus => "restore-census",
            Self::Import => "import",
            Self::Export => "export",
            Self::SaveBeforeCensus => "save-before-census",
            Self::SaveAfterCensus => "save-after-census",
            Self::OwnerLayerEvidence => "owner-layer-evidence",
            Self::OwnerAggregate => "owner-aggregate",
        }
    }
}

/// Phase for a generated composite that groups one cache owner's workflow steps.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QualificationCacheCompositePhase {
    /// Runtime observation, binding, restore, and import before task execution.
    Prepare,
    /// Export and inspect state immediately before one workflow-scope save.
    BeforeSave,
    /// Inspect state and record evidence immediately after one workflow-scope save.
    AfterSave,
}

impl QualificationCacheCompositePhase {
    /// Fixed ID spelling for this composite phase.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Prepare => "prepare",
            Self::BeforeSave => "before-save",
            Self::AfterSave => "after-save",
        }
    }
}

/// Typed ID for a composite action that groups one validated cache owner.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QualificationCacheCompositeId {
    phase: QualificationCacheCompositePhase,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    layer: Option<QualificationCacheLayer>,
    owner_tag: u64,
}

/// Closed outputs passed from the prepare composite to the finish composite.
#[must_use]
pub fn qualification_cache_composite_output_names() -> Vec<String> {
    let layers = [
        QualificationCacheLayer::MbxObjects,
        QualificationCacheLayer::CargoSources,
        QualificationCacheLayer::MiseTools,
        QualificationCacheLayer::TofuProviders,
        QualificationCacheLayer::TaskResult,
    ];
    let mut names = Vec::new();
    for layer in layers {
        let layer = layer.as_str();
        names.extend([
            format!("cache_{layer}_runtime_before"),
            format!("cache_{layer}_roots"),
            format!("cache_{layer}_active"),
            format!("cache_{layer}_save_key"),
            format!("cache_{layer}_save_stage_path"),
            format!("cache_{layer}_restore_result"),
        ]);
        if layer != QualificationCacheLayer::TaskResult.as_str() {
            names.push(format!("cache_{layer}_runtime_after"));
        }
        for field in [
            "phase",
            "key",
            "status",
            "admitted",
            "cache_id",
            "cache_key",
            "cache_ref",
            "cache_version",
            "cache_size",
            "action_hit",
            "action_matched_key",
            "restore_key",
        ] {
            names.push(format!("cache_{layer}_restore_census_{field}"));
        }
    }
    names.sort();
    names
}

/// Closed inputs passed to one layer's before-save composite.
#[must_use]
pub fn qualification_cache_before_save_output_names(layer: QualificationCacheLayer) -> Vec<String> {
    let prefix = format!("cache_{}", layer.as_str());
    let mut names = vec![
        format!("{prefix}_export_result"),
        format!("{prefix}_state_digest"),
        format!("{prefix}_save_requested"),
    ];
    for field in [
        "phase",
        "key",
        "status",
        "admitted",
        "cache_id",
        "cache_key",
        "cache_ref",
        "cache_version",
        "cache_size",
    ] {
        names.push(format!("{prefix}_save_before_census_{field}"));
    }
    names.sort();
    names
}

/// Closed prelude values needed by one layer's before-save composite.
#[must_use]
pub fn qualification_cache_layer_prepare_input_names(
    _layer: QualificationCacheLayer,
) -> Vec<String> {
    vec!["prepare_outputs".to_owned()]
}

/// Closed inputs passed to one layer's after-save composite.
#[must_use]
pub fn qualification_cache_after_save_input_names(layer: QualificationCacheLayer) -> Vec<String> {
    vec![
        "before_outputs".to_owned(),
        "prepare_outputs".to_owned(),
        format!("cache_{}_save_action_outcome", layer.as_str()),
    ]
}

/// Closed output exposed by one layer's after-save composite.
#[must_use]
pub fn qualification_cache_after_save_output_names(layer: QualificationCacheLayer) -> Vec<String> {
    vec![format!("cache_{}_owner_evidence_path", layer.as_str())]
}

impl QualificationCacheCompositeId {
    /// Build a composite identity from one validated generated job ID.
    ///
    /// # Errors
    ///
    /// Returns an error when the owner ID is not canonical.
    pub fn new(
        phase: QualificationCacheCompositePhase,
        owner_id: &str,
    ) -> Result<Self, crate::errors::ContractError> {
        crate::workflow::validate_job_id(owner_id)?;
        if phase != QualificationCacheCompositePhase::Prepare {
            return Err(crate::errors::ContractError::identity(
                "step.id",
                "qualification_cache_composite_layer_required".to_owned(),
            ));
        }
        Ok(Self::new_inner(phase, None, owner_id))
    }

    /// Build one layer-scoped boundary identity for a generated job.
    ///
    /// # Errors
    ///
    /// Returns an error when the phase, layer, or owner ID is invalid.
    pub fn for_layer(
        phase: QualificationCacheCompositePhase,
        layer: QualificationCacheLayer,
        owner_id: &str,
    ) -> Result<Self, crate::errors::ContractError> {
        crate::workflow::validate_job_id(owner_id)?;
        if phase == QualificationCacheCompositePhase::Prepare {
            return Err(crate::errors::ContractError::identity(
                "step.id",
                "qualification_cache_prepare_must_be_owner_scoped".to_owned(),
            ));
        }
        Ok(Self::new_inner(phase, Some(layer), owner_id))
    }

    fn new_inner(
        phase: QualificationCacheCompositePhase,
        layer: Option<QualificationCacheLayer>,
        owner_id: &str,
    ) -> Self {
        let digest = blake3::hash(owner_id.as_bytes());
        let mut tag = [0_u8; 8];
        tag.copy_from_slice(&digest.as_bytes()[..8]);
        Self {
            phase,
            layer,
            owner_tag: u64::from_be_bytes(tag),
        }
    }

    /// Render the validated ID for a GitHub Actions step.
    #[must_use]
    pub fn as_string(self) -> String {
        match self.layer {
            Some(layer) => format!(
                "qualification-cache-{}-{}-{:016x}",
                self.phase.as_str(),
                layer.as_str(),
                self.owner_tag
            ),
            None => format!(
                "qualification-cache-{}-{:016x}",
                self.phase.as_str(),
                self.owner_tag
            ),
        }
    }

    /// Phase for this composite identity.
    #[must_use]
    pub const fn phase(self) -> QualificationCacheCompositePhase {
        self.phase
    }

    /// Layer for this boundary, when it surrounds one layer's save.
    #[must_use]
    pub const fn layer(self) -> Option<QualificationCacheLayer> {
        self.layer
    }

    /// Check whether a prepare and finish identity belong to the same owner.
    #[must_use]
    pub const fn same_owner(self, other: Self) -> bool {
        self.owner_tag == other.owner_tag
    }

    /// Check whether this ID belongs to the supplied generated job ID.
    #[must_use]
    pub fn belongs_to(self, owner_id: &str) -> bool {
        let digest = blake3::hash(owner_id.as_bytes());
        let mut tag = [0_u8; 8];
        tag.copy_from_slice(&digest.as_bytes()[..8]);
        self.owner_tag == u64::from_be_bytes(tag)
    }
}
