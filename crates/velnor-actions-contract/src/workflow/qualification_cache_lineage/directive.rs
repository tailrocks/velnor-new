//! Plan-derived cache permissions consumed by cache-layer producers.

use crate::errors::ContractError;
use crate::workflow::plan::Plan;

use super::MAX_QUALIFICATION_CACHE_LANES;
use super::identity::{
    BoundQualificationCacheKeys, QualificationCacheLayer, QualificationCacheRestoreExpectation,
    QualificationRuntimeIdentity, configuration_commitment, key_for_slot,
    validate_runtime_identity,
};
use super::receipt::QualificationCacheAdmission;
mod owner;

use owner::{lane_directive, owner_directive};

pub use super::directive_types::{
    QualificationCacheDirective, QualificationCacheLaneDirective,
    QualificationCacheLaneLayerDirective, QualificationCacheLayerDirective,
    QualificationCacheOwnerDirective, QualificationCacheRestoreDirective,
    QualificationCacheRestorePolicy, QualificationCacheSavePolicy,
};

/// Output name for canonical phase/lane/layer cache directives.
pub const QUALIFICATION_CACHE_DIRECTIVES_OUTPUT: &str = "qualification_cache_directives";

impl QualificationCacheDirective {
    /// Build directives from a fully validated plan and admitted lineage.
    ///
    /// Non-qualification plans return None. Cold and Control reject any
    /// predecessor admission; lineage phases reject a missing one.
    /// # Errors
    pub fn for_plan(
        plan: &Plan,
        admission: Option<&QualificationCacheAdmission>,
    ) -> Result<Option<Self>, ContractError> {
        plan.validate()?;
        let Some(context) = &plan.qualification else {
            return if admission.is_some() {
                Err(invalid("admission_for_non_qualification_plan"))
            } else {
                Ok(None)
            };
        };
        // Plan::validate has already compared this immutable context with the
        // plan. The orchestrator compares the ref with the configured refs
        // before planning. This stage keeps that ref for separate cache keys.
        if plan.matrix.include.len() > MAX_QUALIFICATION_CACHE_LANES {
            return Err(invalid("too_many_lanes"));
        }
        match (context.phase.predecessor(), admission) {
            (None, None) => {}
            (None, Some(_)) => return Err(invalid("unexpected_predecessor_admission")),
            (Some(_), None) => return Err(invalid("missing_predecessor_admission")),
            (Some(_), Some(value)) => value.validate_for_plan(plan)?,
        }
        let config_digest = configuration_commitment(plan)?;
        let mut owner_ids = plan
            .matrix
            .include
            .iter()
            .map(|entry| entry.job_id.clone())
            .collect::<Vec<_>>();
        owner_ids.sort();
        owner_ids.dedup();
        let mut owners = owner_ids
            .iter()
            .map(|owner_id| owner_directive(plan, owner_id, admission))
            .collect::<Result<Vec<_>, _>>()?;
        owners.sort_by(|left, right| left.owner_id.cmp(&right.owner_id));
        let mut lanes = plan
            .matrix
            .include
            .iter()
            .map(|entry| lane_directive(entry, &owners))
            .collect::<Result<Vec<_>, _>>()?;
        lanes.sort_by(|left, right| left.matrix_key.cmp(&right.matrix_key));
        let (predecessor_receipt_digest, source_delta) = if let Some(value) = admission {
            (Some(value.receipt_digest()?), value.source_delta().cloned())
        } else {
            (None, None)
        };
        Ok(Some(Self {
            schema: 4,
            campaign: context.campaign.clone(),
            phase: context.phase,
            plan_id: plan.plan_id.clone(),
            run_key: plan.run_key.clone(),
            configuration_digest: config_digest,
            predecessor: context.predecessor,
            predecessor_receipt_digest,
            source_delta,
            owners,
            lanes,
        }))
    }

    /// Find one lane/layer instruction without interpreting phase names.
    #[must_use]
    pub fn layer(
        &self,
        matrix_key: &str,
        layer: QualificationCacheLayer,
    ) -> Option<&QualificationCacheLayerDirective> {
        self.owners
            .iter()
            .find(|owner| {
                owner
                    .members
                    .iter()
                    .any(|member| member.matrix_key == matrix_key)
            })?
            .layers
            .iter()
            .find(|record| record.layer == layer)
    }

    /// Return the cache layer assigned to this generated job ID.
    #[must_use]
    pub fn owner_layer(
        &self,
        owner_id: &str,
        layer: QualificationCacheLayer,
    ) -> Option<&QualificationCacheLayerDirective> {
        self.owners
            .iter()
            .find(|owner| owner.owner_id == owner_id)?
            .layers
            .iter()
            .find(|record| record.layer == layer)
    }

    /// Rebuild this serialized directive from its plan and admission, then bind runtime identity.
    ///
    /// The serialized fields alone never authorize cache access. Any field
    /// that differs from the plan-derived directive is rejected before keys
    /// are returned.
    /// # Errors
    pub fn bind_runtime_owner(
        &self,
        plan: &Plan,
        admission: Option<&QualificationCacheAdmission>,
        owner_id: &str,
        layer: QualificationCacheLayer,
        evidence: &QualificationRuntimeIdentity,
    ) -> Result<BoundQualificationCacheKeys, ContractError> {
        let expected = Self::for_plan(plan, admission)?
            .ok_or_else(|| invalid("directive_for_non_qualification_plan"))?;
        if self != &expected {
            return Err(invalid("serialized_directive_does_not_match_plan"));
        }
        self.bind_validated_runtime(owner_id, layer, evidence)
    }

    fn bind_validated_runtime(
        &self,
        owner_id: &str,
        layer: QualificationCacheLayer,
        evidence: &QualificationRuntimeIdentity,
    ) -> Result<BoundQualificationCacheKeys, ContractError> {
        let directive = self
            .owner_layer(owner_id, layer)
            .ok_or_else(|| invalid("owner_or_layer_not_in_directive"))?;
        if !directive.active
            || directive.restore_policy == QualificationCacheRestorePolicy::Disabled
        {
            return Ok(BoundQualificationCacheKeys {
                restore: None,
                save_key: None,
                save_if_state_changes: false,
                expected_prior_state_digest: None,
                runtime_identity_digest: None,
            });
        }
        let requirements = directive
            .runtime
            .as_ref()
            .ok_or_else(|| invalid("active_layer_runtime_requirements_missing"))?;
        let runtime_digest = validate_runtime_identity(requirements, evidence)?;
        if directive
            .expected_runtime
            .as_ref()
            .is_some_and(|expected| expected != evidence)
        {
            return Err(invalid("runtime_identity_changed_since_predecessor"));
        }
        let restore = directive
            .restore
            .as_ref()
            .map(|restore| {
                let key = key_for_slot(
                    &self.campaign,
                    layer,
                    &directive.identity_digest,
                    &runtime_digest,
                    restore.slot,
                )?;
                if restore
                    .expected_cache
                    .as_ref()
                    .is_some_and(|archive| archive.key != key)
                {
                    return Err(invalid("restore_archive_key_mismatch"));
                }
                Ok(QualificationCacheRestoreExpectation {
                    requested_key: key,
                    expected_cache: restore.expected_cache.clone(),
                })
            })
            .transpose()?;
        let save_key = directive
            .save_policy
            .slot()
            .map(|slot| {
                key_for_slot(
                    &self.campaign,
                    layer,
                    &directive.identity_digest,
                    &runtime_digest,
                    slot,
                )
            })
            .transpose()?;
        Ok(BoundQualificationCacheKeys {
            restore,
            save_key,
            save_if_state_changes: directive.save_policy.conditional(),
            expected_prior_state_digest: directive.expected_prior_state_digest.clone(),
            runtime_identity_digest: Some(runtime_digest),
        })
    }
}

fn invalid(reason: &str) -> ContractError {
    ContractError::identity("qualification.cache_directive", reason)
}
