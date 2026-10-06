//! Serialized, closed qualification cache directive records.

use serde::{Deserialize, Serialize};

use crate::workflow::{QualificationPhase, QualificationRunRef};

use super::identity::{
    QualificationCacheLayer, QualificationCacheSlot, QualificationRuntimeIdentity,
    QualificationRuntimeIdentityRequirements,
};
use super::receipt::{QualificationCacheBackendEntry, QualificationSourceDelta};

/// Restore bytes only after observed action and backend evidence are admitted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QualificationCacheRestorePolicy {
    /// Require post-restore matched-key and backend-object admission.
    AdmissionGated,
    /// No cache restore is allowed.
    Disabled,
}

/// Typed save authorization for one phase and cache layer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QualificationCacheSavePolicy {
    /// No cache save is permitted.
    Disabled,
    /// Create the cold K1 snapshot.
    K1,
    /// Create K2 only when the observed useful layer state changed.
    K2WhenStateChanges,
    /// Create K3 only when the observed useful layer state changed.
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

/// Required predecessor archive, or required miss, for one restore request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QualificationCacheRestoreDirective {
    /// Logical slot used to derive the runner-bound restore key.
    pub slot: QualificationCacheSlot,
    /// Exact immutable object admitted from the predecessor receipt; None
    /// means the action must miss and any staged bytes must be discarded.
    pub expected_cache: Option<QualificationCacheBackendEntry>,
}

/// One lane/layer's exact restore and save boundary.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QualificationCacheLayerDirective {
    /// Closed cache layer.
    pub layer: QualificationCacheLayer,
    /// Whether this layer applies to the lane and phase.
    pub active: bool,
    /// Stable configuration/workspace/lane/cache-format identity commitment.
    pub identity_digest: String,
    /// Required actual runner facts before key use.
    pub runtime: QualificationRuntimeIdentityRequirements,
    /// Prior admitted runtime facts; current observations must equal these.
    pub expected_runtime: Option<QualificationRuntimeIdentity>,
    /// Exact restore expectation; stage action output before import.
    pub restore: Option<QualificationCacheRestoreDirective>,
    /// Closed restore policy.
    pub restore_policy: QualificationCacheRestorePolicy,
    /// Closed save policy; conditional policies compare state digests first.
    pub save_policy: QualificationCacheSavePolicy,
    /// Prior actual useful payload digest for conditional save decisions.
    pub expected_prior_state_digest: Option<String>,
}

/// One matrix lane's sorted layer directives.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QualificationCacheLaneDirective {
    /// Stable plan matrix key.
    pub matrix_key: String,
    /// Stable stack and task identity.
    pub stack_id: String,
    /// Stable task-group identity.
    pub task_id: String,
    /// All layer directives, including the disabled `TaskResult` layer.
    pub layers: Vec<QualificationCacheLayerDirective>,
}

/// Bounded phase directive map written as one job output.
///
/// Serialized maps are untrusted until they are reconstructed against their
/// plan and admitted predecessor before runtime key binding.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QualificationCacheDirective {
    /// Directive schema version.
    pub schema: u32,
    /// Validated campaign namespace.
    pub campaign: String,
    /// Current validated phase.
    pub phase: QualificationPhase,
    /// Current plan ID, bound to its run and attempt.
    pub plan_id: String,
    /// Current run key.
    pub run_key: String,
    /// Stable plan configuration and obligation-set identity.
    pub configuration_digest: String,
    /// Exact run/attempt whose receipt admitted this directive.
    pub predecessor: Option<QualificationRunRef>,
    /// Digest of the admitted predecessor receipt.
    pub predecessor_receipt_digest: Option<String>,
    /// Source change proof required by `UsefulDelta`.
    pub source_delta: Option<QualificationSourceDelta>,
    /// Sorted lane and layer records.
    pub lanes: Vec<QualificationCacheLaneDirective>,
}
