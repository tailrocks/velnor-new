//! This module builds cache directives for generated jobs.

use crate::errors::ContractError;
use crate::workflow::QualificationPhase;
use crate::workflow::plan::Plan;

use super::super::identity::{
    QualificationCacheLayer, QualificationCacheOwner, QualificationCacheSlot, identity_commitment,
    owner_for_plan, owner_layer_identity, owner_runtime_requirements, runtime_requirements,
};
use super::super::receipt::{QualificationCacheAdmission, layer_applies};
use super::{
    QualificationCacheLaneDirective, QualificationCacheLaneLayerDirective,
    QualificationCacheLayerDirective, QualificationCacheOwnerDirective,
    QualificationCacheRestoreDirective, QualificationCacheRestorePolicy,
    QualificationCacheSavePolicy, invalid,
};

pub(super) fn owner_directive(
    plan: &Plan,
    owner_id: &str,
    admission: Option<&QualificationCacheAdmission>,
) -> Result<QualificationCacheOwnerDirective, ContractError> {
    let owner = owner_for_plan(plan, owner_id)?;
    let layers = [
        QualificationCacheLayer::MbxObjects,
        QualificationCacheLayer::CargoSources,
        QualificationCacheLayer::MiseTools,
        QualificationCacheLayer::TofuProviders,
        QualificationCacheLayer::TaskResult,
    ]
    .into_iter()
    .map(|layer| owner_layer_directive(plan, &owner, layer, admission))
    .collect::<Result<Vec<_>, _>>()?;
    Ok(QualificationCacheOwnerDirective {
        owner_id: owner.owner_id,
        platform_id: owner.platform_id,
        member_digest: owner.member_digest,
        members: owner.members,
        layers,
    })
}

pub(super) fn lane_directive(
    entry: &crate::workflow::MatrixEntry,
    owners: &[QualificationCacheOwnerDirective],
) -> Result<QualificationCacheLaneDirective, ContractError> {
    let owner = owners
        .iter()
        .find(|owner| owner.owner_id == entry.job_id)
        .ok_or_else(|| invalid("matrix_member_owner_missing"))?;
    let layers = owner
        .layers
        .iter()
        .map(|layer| QualificationCacheLaneLayerDirective {
            layer: layer.layer,
            active: layer.active,
            identity_digest: layer.identity_digest.clone(),
            owner_id: layer.active.then(|| owner.owner_id.clone()),
        })
        .collect();
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
    owner_id: &str,
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
    if active && runtime.is_none() {
        return Err(invalid("active_layer_runtime_requirements_missing"));
    }
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

    let restore = phase_restore(phase, admission, owner_id, layer)?;
    let (expected_runtime, expected_prior_state_digest) = match admission {
        None => (None, None),
        Some(value) => {
            let prior = value.layer_receipt(owner_id, layer)?;
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

fn owner_layer_directive(
    plan: &Plan,
    owner: &QualificationCacheOwner,
    layer: QualificationCacheLayer,
    admission: Option<&QualificationCacheAdmission>,
) -> Result<QualificationCacheLayerDirective, ContractError> {
    let entry = owner
        .entries
        .first()
        .ok_or_else(|| invalid("owner_has_no_matrix_members"))?;
    let mut directive = layer_directive(plan, entry, &owner.owner_id, layer, admission)?;
    directive.identity_digest = owner_layer_identity(plan, owner, layer)?;
    directive.runtime = owner_runtime_requirements(plan, owner)?;
    directive.active = owner
        .entries
        .iter()
        .any(|member| layer_applies(member, layer))
        && layer != QualificationCacheLayer::TaskResult
        && plan
            .qualification
            .as_ref()
            .is_some_and(|context| context.phase != QualificationPhase::Control);
    if !directive.active {
        directive.runtime = None;
        directive.expected_runtime = None;
        directive.restore = None;
        directive.restore_policy = QualificationCacheRestorePolicy::Disabled;
        directive.save_policy = QualificationCacheSavePolicy::Disabled;
        directive.expected_prior_state_digest = None;
    }
    Ok(directive)
}

fn phase_restore(
    phase: QualificationPhase,
    admission: Option<&QualificationCacheAdmission>,
    owner_id: &str,
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
                .saved_entry(owner_id, layer, QualificationCacheSlot::K1)?;
            Ok(QualificationCacheRestoreDirective {
                slot: QualificationCacheSlot::K1,
                expected_cache: Some(entry),
            })
        }
        QualificationPhase::Third | QualificationPhase::UsefulDelta => {
            let (slot, entry) = admission
                .ok_or_else(|| invalid("missing_predecessor_admission"))?
                .latest_saved_entry(owner_id, layer)?;
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
