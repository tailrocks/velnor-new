//! Worker-produced cache facts without backend admission claims.

use serde::{Deserialize, Serialize};
use velnor_actions_contract::strict_json::parse_strict_json_bytes;
use velnor_actions_contract::workflow::{
    QualificationCacheLayer, QualificationCacheOwnerMemberRef, QualificationCacheRestoreResult,
    QualificationCacheSaveActionResult, QualificationRuntimeIdentity,
};
use velnor_actions_contract::{canonical::Digest, canonical_json_bytes, workflow::validate_job_id};

use crate::OrchestratorError;
use crate::internal::internal;

const WORKER_SCHEMA: u32 = 1;
const WORKER_KIND: &str = "qualification_cache_worker_owner_facts";
const MAX_BYTES: usize = 262_144;
const LAYERS: [QualificationCacheLayer; 5] = [
    QualificationCacheLayer::MbxObjects,
    QualificationCacheLayer::CargoSources,
    QualificationCacheLayer::MiseTools,
    QualificationCacheLayer::TofuProviders,
    QualificationCacheLayer::TaskResult,
];

/// Untrusted facts that a worker reports for one cache owner.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QualificationCacheWorkerOwnerFacts {
    /// Closed worker-facts schema version.
    pub schema: u32,
    /// Distinguishes worker facts from the final trusted receipt.
    pub kind: String,
    /// Exact generated job that owns the physical cache layers.
    pub owner_id: String,
    /// Planned platform identity shared by owner members.
    pub platform_id: String,
    /// Digest of all sorted owner members.
    pub member_digest: String,
    /// Complete sorted owner membership.
    pub members: Vec<QualificationCacheOwnerMemberRef>,
    /// One worker-facts record for each closed cache layer, in fixed order.
    pub layers: Vec<QualificationCacheWorkerLayerFacts>,
}

/// Facts that a worker reports for one layer. Required supplies backend observations.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QualificationCacheWorkerLayerFacts {
    /// The owner identity repeated in this layer record.
    pub owner_id: String,
    /// Repeated planned platform binding.
    pub platform_id: String,
    /// Repeated complete member-set digest.
    pub member_digest: String,
    /// Closed cache layer.
    pub layer: QualificationCacheLayer,
    /// Whether the worker reports this layer as active.
    pub active: bool,
    /// Owner-layer identity digest.
    pub identity_digest: String,
    /// Runtime identity observed by the worker.
    pub runtime_identity: Option<QualificationRuntimeIdentity>,
    /// Useful-state digest captured after the job.
    pub state_digest: Option<String>,
    /// Cache action facts, without an API census.
    pub restore: QualificationCacheWorkerRestoreFacts,
    /// Save action facts, without before/after API observations.
    pub save: QualificationCacheWorkerSaveFacts,
}

/// Restore action facts emitted by the worker.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QualificationCacheWorkerRestoreFacts {
    /// The restore key that the worker reports for this layer.
    pub requested_key: Option<String>,
    /// Exact key reported by the cache action, if it reports a hit.
    pub matched_key: Option<String>,
    /// Cache action result.
    pub result: QualificationCacheRestoreResult,
}

/// Save action facts emitted by the worker.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QualificationCacheWorkerSaveFacts {
    /// Exact save key when a save action was requested.
    pub requested_key: Option<String>,
    /// Save action result, without asserting archive creation.
    pub action: QualificationCacheSaveActionResult,
}

impl QualificationCacheWorkerOwnerFacts {
    /// Version of the worker-only wire schema.
    pub const SCHEMA: u32 = WORKER_SCHEMA;
    /// Discriminator that prevents parsing worker facts as a final receipt.
    pub const KIND: &'static str = WORKER_KIND;

    /// Parse canonical bounded worker facts and reject final-receipt fields.
    ///
    /// # Errors
    /// Returns an error for malformed, noncanonical, oversized, or invalid facts.
    pub fn parse_canonical(bytes: &[u8]) -> Result<Self, OrchestratorError> {
        if bytes.len() > MAX_BYTES {
            return Err(internal("qualification_cache_worker_facts_oversize"));
        }
        let value = parse_strict_json_bytes(bytes, MAX_BYTES)
            .map_err(|_| internal("qualification_cache_worker_facts_json"))?;
        let facts: Self = serde_json::from_value(value)
            .map_err(|_| internal("qualification_cache_worker_facts_shape"))?;
        facts.validate()?;
        let canonical = canonical_json_bytes(&facts).map_err(crate::internal::internal_contract)?;
        if canonical != bytes {
            return Err(internal("qualification_cache_worker_facts_noncanonical"));
        }
        Ok(facts)
    }

    /// Validate the closed schema and repeated owner/layer bindings.
    ///
    /// # Errors
    /// Returns an error when facts have an incomplete or inconsistent shape.
    pub fn validate(&self) -> Result<(), OrchestratorError> {
        validate_job_id(&self.owner_id).map_err(crate::internal::internal_contract)?;
        if self.schema != WORKER_SCHEMA
            || self.kind != WORKER_KIND
            || self.platform_id.is_empty()
            || !is_typed_digest(&self.member_digest)
            || self.members.is_empty()
            || self.members.len() > 256
            || self.layers.len() != LAYERS.len()
        {
            return Err(internal("qualification_cache_worker_facts_identity"));
        }
        let mut prior_matrix_key: Option<&str> = None;
        for member in &self.members {
            if member.matrix_key.is_empty()
                || member.stack_id.is_empty()
                || member.task_id.is_empty()
                || !is_typed_digest(&member.task_digest)
                || !is_typed_digest(&member.input_digest)
                || prior_matrix_key.is_some_and(|prior| prior >= member.matrix_key.as_str())
            {
                return Err(internal("qualification_cache_worker_facts_members"));
            }
            prior_matrix_key = Some(&member.matrix_key);
        }
        for (layer, expected) in self.layers.iter().zip(LAYERS) {
            if layer.owner_id != self.owner_id
                || layer.platform_id != self.platform_id
                || layer.member_digest != self.member_digest
                || layer.layer != expected
                || !is_typed_digest(&layer.identity_digest)
            {
                return Err(internal("qualification_cache_worker_facts_layer_binding"));
            }
            layer.validate_shape()?;
        }
        Ok(())
    }
}

impl QualificationCacheWorkerLayerFacts {
    /// Parse one canonical bounded layer record.
    ///
    /// # Errors
    /// Returns an error for malformed, noncanonical, oversized, or inconsistent facts.
    pub fn parse_canonical(bytes: &[u8]) -> Result<Self, OrchestratorError> {
        if bytes.len() > 65_536 {
            return Err(internal("qualification_cache_worker_layer_oversize"));
        }
        let value = parse_strict_json_bytes(bytes, 65_536)
            .map_err(|_| internal("qualification_cache_worker_layer_json"))?;
        let facts: Self = serde_json::from_value(value)
            .map_err(|_| internal("qualification_cache_worker_layer_shape"))?;
        facts.validate_shape()?;
        if canonical_json_bytes(&facts).map_err(crate::internal::internal_contract)? != bytes {
            return Err(internal("qualification_cache_worker_layer_noncanonical"));
        }
        Ok(facts)
    }

    /// Validate fields that do not depend on the authenticated owner directive.
    ///
    /// # Errors
    /// Returns an error if the action and active-state fields conflict.
    pub fn validate_shape(&self) -> Result<(), OrchestratorError> {
        validate_layer(self)
    }
}

fn validate_layer(layer: &QualificationCacheWorkerLayerFacts) -> Result<(), OrchestratorError> {
    let valid_restore = match layer.restore.result {
        QualificationCacheRestoreResult::Disabled => {
            layer.restore.requested_key.is_none() && layer.restore.matched_key.is_none()
        }
        QualificationCacheRestoreResult::Miss => {
            layer.restore.requested_key.is_some() && layer.restore.matched_key.is_none()
        }
        QualificationCacheRestoreResult::Hit => {
            layer.restore.requested_key.is_some() && layer.restore.matched_key.is_some()
        }
    };
    if !valid_restore {
        return Err(internal("qualification_cache_worker_facts_restore"));
    }
    let valid_save = match layer.save.action {
        QualificationCacheSaveActionResult::Disabled
        | QualificationCacheSaveActionResult::NotRequired => layer.save.requested_key.is_none(),
        QualificationCacheSaveActionResult::Succeeded
        | QualificationCacheSaveActionResult::Skipped
        | QualificationCacheSaveActionResult::Failed => layer.save.requested_key.is_some(),
    };
    let active_shape = if layer.active {
        layer.runtime_identity.is_some() && layer.state_digest.is_some()
    } else {
        layer.runtime_identity.is_none()
            && layer.state_digest.is_none()
            && layer.restore.result == QualificationCacheRestoreResult::Disabled
            && layer.save.action == QualificationCacheSaveActionResult::Disabled
    };
    if !valid_save || !active_shape {
        return Err(internal("qualification_cache_worker_facts_layer_shape"));
    }
    if layer
        .state_digest
        .as_ref()
        .is_some_and(|digest| !is_typed_digest(digest))
    {
        return Err(internal("qualification_cache_worker_facts_state_digest"));
    }
    Ok(())
}

fn is_typed_digest(value: &str) -> bool {
    Digest::parse(value).is_ok()
}

#[cfg(test)]
#[path = "qualification_cache_worker_facts_tests.rs"]
mod tests;
