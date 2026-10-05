//! Plan-derived cache permissions consumed by cache-layer producers.

use crate::errors::ContractError;
use crate::workflow::QualificationPhase;
use crate::workflow::plan::Plan;

use super::MAX_QUALIFICATION_CACHE_LANES;
use super::identity::{
    BoundQualificationCacheKeys, QualificationCacheLayer, QualificationCacheRestoreExpectation,
    QualificationCacheSlot, QualificationRuntimeIdentity, configuration_commitment,
    identity_commitment, key_for_slot, runtime_requirements, validate_runtime_identity,
};
use super::receipt::{QualificationCacheAdmission, layer_applies};

pub use super::directive_types::{
    QualificationCacheDirective, QualificationCacheLaneDirective, QualificationCacheLayerDirective,
    QualificationCacheRestoreDirective, QualificationCacheRestorePolicy,
    QualificationCacheSavePolicy,
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
        context.validate_for(&context.default_branch, &context.repository, &plan.head)?;
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
        let mut lanes = plan
            .matrix
            .include
            .iter()
            .map(|entry| lane_directive(plan, entry, admission))
            .collect::<Result<Vec<_>, _>>()?;
        lanes.sort_by(|left, right| left.matrix_key.cmp(&right.matrix_key));
        let (predecessor_receipt_digest, source_delta) = if let Some(value) = admission {
            (Some(value.receipt_digest()?), value.source_delta().cloned())
        } else {
            (None, None)
        };
        Ok(Some(Self {
            schema: 1,
            campaign: context.campaign.clone(),
            phase: context.phase,
            plan_id: plan.plan_id.clone(),
            run_key: plan.run_key.clone(),
            configuration_digest: config_digest,
            predecessor: context.predecessor,
            predecessor_receipt_digest,
            source_delta,
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
        self.lanes
            .iter()
            .find(|lane| lane.matrix_key == matrix_key)?
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
    pub fn bind_runtime(
        &self,
        plan: &Plan,
        admission: Option<&QualificationCacheAdmission>,
        matrix_key: &str,
        layer: QualificationCacheLayer,
        evidence: &QualificationRuntimeIdentity,
    ) -> Result<BoundQualificationCacheKeys, ContractError> {
        let expected = Self::for_plan(plan, admission)?
            .ok_or_else(|| invalid("directive_for_non_qualification_plan"))?;
        if self != &expected {
            return Err(invalid("serialized_directive_does_not_match_plan"));
        }
        self.bind_validated_runtime(matrix_key, layer, evidence)
    }

    fn bind_validated_runtime(
        &self,
        matrix_key: &str,
        layer: QualificationCacheLayer,
        evidence: &QualificationRuntimeIdentity,
    ) -> Result<BoundQualificationCacheKeys, ContractError> {
        let directive = self
            .layer(matrix_key, layer)
            .ok_or_else(|| invalid("lane_or_layer_not_in_directive"))?;
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
        let runtime_digest = validate_runtime_identity(&directive.runtime, evidence)?;
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

fn lane_directive(
    plan: &Plan,
    entry: &crate::workflow::MatrixEntry,
    admission: Option<&QualificationCacheAdmission>,
) -> Result<QualificationCacheLaneDirective, ContractError> {
    let mut layers = Vec::with_capacity(6);
    for layer in [
        QualificationCacheLayer::MbxObjects,
        QualificationCacheLayer::MbxBundle,
        QualificationCacheLayer::CargoSources,
        QualificationCacheLayer::MiseTools,
        QualificationCacheLayer::TofuProviders,
        QualificationCacheLayer::TaskResult,
    ] {
        layers.push(layer_directive(plan, entry, layer, admission)?);
    }
    Ok(QualificationCacheLaneDirective {
        matrix_key: entry.matrix_key.clone(),
        stack_id: entry.stack_id.clone(),
        task_id: entry.task_id.clone(),
        layers,
    })
}

fn layer_directive(
    plan: &Plan,
    entry: &crate::workflow::MatrixEntry,
    layer: QualificationCacheLayer,
    admission: Option<&QualificationCacheAdmission>,
) -> Result<QualificationCacheLayerDirective, ContractError> {
    let context = plan
        .qualification
        .as_ref()
        .ok_or_else(|| invalid("missing_qualification_context"))?;
    let phase = context.phase;
    let active = layer_applies(entry, layer)
        && layer != QualificationCacheLayer::TaskResult
        && phase != QualificationPhase::Control;
    let identity = identity_commitment(plan, entry, layer)?;
    let runtime = runtime_requirements(plan, entry)?;
    if !active {
        return Ok(QualificationCacheLayerDirective {
            layer,
            active: false,
            identity_digest: identity,
            runtime,
            expected_runtime: None,
            restore: None,
            restore_policy: QualificationCacheRestorePolicy::Disabled,
            save_policy: QualificationCacheSavePolicy::Disabled,
            expected_prior_state_digest: None,
        });
    }

    let restore = phase_restore(phase, admission, &entry.matrix_key, layer)?;
    let (expected_runtime, expected_prior_state_digest) = match admission {
        None => (None, None),
        Some(value) => {
            let prior = value.layer_receipt(&entry.matrix_key, layer)?;
            let runtime = prior
                .runtime_identity
                .clone()
                .ok_or_else(|| invalid("predecessor_runtime_identity_missing"))?;
            let state = prior
                .state_digest
                .clone()
                .ok_or_else(|| invalid("predecessor_layer_state_missing"))?;
            (Some(runtime), Some(state))
        }
    };
    Ok(QualificationCacheLayerDirective {
        layer,
        active: true,
        identity_digest: identity,
        runtime,
        expected_runtime,
        restore_policy: QualificationCacheRestorePolicy::AdmissionGated,
        restore: Some(restore),
        save_policy: phase_save_policy(phase),
        expected_prior_state_digest,
    })
}

fn phase_restore(
    phase: QualificationPhase,
    admission: Option<&QualificationCacheAdmission>,
    matrix_key: &str,
    layer: QualificationCacheLayer,
) -> Result<QualificationCacheRestoreDirective, ContractError> {
    match phase {
        QualificationPhase::Cold => Ok(QualificationCacheRestoreDirective {
            slot: QualificationCacheSlot::K1,
            expected_cache: None,
        }),
        QualificationPhase::Warm => {
            let entry = admission
                .ok_or_else(|| invalid("missing_predecessor_admission"))?
                .saved_entry(matrix_key, layer, QualificationCacheSlot::K1)?;
            Ok(QualificationCacheRestoreDirective {
                slot: QualificationCacheSlot::K1,
                expected_cache: Some(entry),
            })
        }
        QualificationPhase::Third | QualificationPhase::UsefulDelta => {
            let (slot, entry) = admission
                .ok_or_else(|| invalid("missing_predecessor_admission"))?
                .latest_saved_entry(matrix_key, layer)?;
            Ok(QualificationCacheRestoreDirective {
                slot,
                expected_cache: Some(entry),
            })
        }
        QualificationPhase::Control => Err(invalid("control_cache_layer_disabled")),
    }
}

fn phase_save_policy(phase: QualificationPhase) -> QualificationCacheSavePolicy {
    match phase {
        QualificationPhase::Cold => QualificationCacheSavePolicy::K1,
        QualificationPhase::Warm => QualificationCacheSavePolicy::K2WhenStateChanges,
        QualificationPhase::Third | QualificationPhase::Control => {
            QualificationCacheSavePolicy::Disabled
        }
        QualificationPhase::UsefulDelta => QualificationCacheSavePolicy::K3WhenStateChanges,
    }
}

fn invalid(reason: &str) -> ContractError {
    ContractError::identity("qualification.cache_directive", reason)
}
