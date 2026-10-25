use serde::{Deserialize, Serialize};

use crate::workflow::{QualificationPhase, QualificationRunRef};

use super::identity::{
    QualificationCacheLayer, QualificationCacheSlot, QualificationRuntimeIdentity,
    QualificationRuntimeIdentityRequirements,
};
use super::receipt::{QualificationCacheBackendEntry, QualificationSourceDelta};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QualificationCacheRestorePolicy {
    AdmissionGated,

    Disabled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QualificationCacheSavePolicy {
    Disabled,

    K1,

    K2WhenStateChanges,

    K3WhenStateChanges,
}

impl QualificationCacheSavePolicy {
    pub(super) fn slot(self) -> Option<QualificationCacheSlot> {
        match self {
            Self::Disabled => None,
            Self::K1 => Some(QualificationCacheSlot::K1),
            Self::K2WhenStateChanges => Some(QualificationCacheSlot::K2),
            Self::K3WhenStateChanges => Some(QualificationCacheSlot::K3),
        }
    }

    pub(super) fn conditional(self) -> bool {
        matches!(self, Self::K2WhenStateChanges | Self::K3WhenStateChanges)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QualificationCacheRestoreDirective {
    pub slot: QualificationCacheSlot,

    pub expected_cache: Option<QualificationCacheBackendEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QualificationCacheLayerDirective {
    pub layer: QualificationCacheLayer,

    pub active: bool,

    pub identity_digest: String,

    pub runtime: QualificationRuntimeIdentityRequirements,

    pub expected_runtime: Option<QualificationRuntimeIdentity>,

    pub restore: Option<QualificationCacheRestoreDirective>,

    pub restore_policy: QualificationCacheRestorePolicy,

    pub save_policy: QualificationCacheSavePolicy,

    pub expected_prior_state_digest: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QualificationCacheLaneDirective {
    pub matrix_key: String,

    pub stack_id: String,

    pub task_id: String,

    pub layers: Vec<QualificationCacheLayerDirective>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QualificationCacheDirective {
    pub schema: u32,

    pub campaign: String,

    pub phase: QualificationPhase,

    pub plan_id: String,

    pub run_key: String,

    pub configuration_digest: String,

    pub predecessor: Option<QualificationRunRef>,

    pub predecessor_receipt_digest: Option<String>,

    pub source_delta: Option<QualificationSourceDelta>,

    pub lanes: Vec<QualificationCacheLaneDirective>,
}
