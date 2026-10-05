use std::error::Error;

use super::*;
use crate::canonical::{canonical_json_bytes, digest_b3};
use crate::workflow::{
    QualificationCacheArtifact, QualificationCacheBackendEntry,
    QualificationCacheBackendObservation, QualificationCacheLaneReceipt, QualificationCacheLayer,
    QualificationCacheLayerReceipt, QualificationCacheReceipt,
    QualificationCacheReceiptArtifactDocument, QualificationCacheReceiptLink,
    QualificationCacheRestore, QualificationCacheRestoreResult, QualificationCacheSave,
    QualificationCacheSaveActionResult, QualificationPhase, QualificationRunRef,
    QualificationSourceDelta,
};

use super::directive_tests::{qualification_plan, runtime_identity};

#[path = "qualification_cache_lineage_metadata_fixture.rs"]
mod metadata_fixture;
use self::metadata_fixture::{
    artifact_metadata, dispatch, producer_context, run_metadata, run_ref,
};

type TestResult<T = ()> = Result<T, Box<dyn Error>>;

pub(super) struct BuiltNode {
    pub(super) value: serde_json::Value,
    pub(super) artifact: QualificationCacheArtifact,
    pub(super) receipt: QualificationCacheReceipt,
}

pub(super) fn phase_plan(
    phase: QualificationPhase,
    run_id: u64,
    source_sha: &str,
    predecessor_run_id: u64,
) -> Result<crate::workflow::Plan, crate::ContractError> {
    qualification_plan(
        phase,
        run_id,
        source_sha,
        Some(QualificationRunRef {
            run_id: predecessor_run_id,
            run_attempt: 1,
        }),
    )
}

pub(super) fn completed_node(
    plan: &crate::workflow::Plan,
    previous: Option<&BuiltNode>,
    source_delta: Option<QualificationSourceDelta>,
) -> TestResult<BuiltNode> {
    let previous_admission = previous
        .map(|node| admission_for(node, source_delta.as_ref()))
        .transpose()?;
    let producer = producer_context(plan)?;
    let metadata = run_metadata(plan)?;
    let receipt = completed_receipt(plan, previous_admission.as_ref(), previous, source_delta)?;
    let document = QualificationCacheReceiptArtifactDocument {
        schema: 1,
        producer,
        receipt: receipt.clone(),
    };
    let bytes = document.to_bounded_json(plan, &metadata, previous_admission.as_ref())?;
    let artifact = artifact_metadata(plan, bytes.len())?;
    let value = serde_json::json!({
        "metadata": metadata,
        "artifact": artifact.clone(),
        "producer": document.producer,
        "receipt": document.receipt,
        "previous": previous.map(|node| node.value.clone()),
    });
    Ok(BuiltNode {
        value,
        artifact,
        receipt,
    })
}

fn completed_receipt(
    plan: &crate::workflow::Plan,
    admission: Option<&QualificationCacheAdmission>,
    previous: Option<&BuiltNode>,
    source_delta: Option<QualificationSourceDelta>,
) -> TestResult<QualificationCacheReceipt> {
    let context = dispatch(plan)?;
    let directive = QualificationCacheDirective::for_plan(plan, admission)?
        .ok_or_else(|| std::io::Error::other("qualification directive missing"))?;
    let lanes = plan
        .matrix
        .include
        .iter()
        .map(|entry| lane_receipt(plan, entry, &directive, admission))
        .collect::<TestResult<Vec<_>>>()?;
    let predecessor = previous.map(receipt_link).transpose()?;
    Ok(QualificationCacheReceipt {
        schema: 1,
        plan_id: plan.plan_id.clone(),
        run: run_ref(context),
        campaign: context.campaign.clone(),
        phase: context.phase,
        source_sha: context.source_sha.clone(),
        configuration_digest: super::identity::configuration_commitment(plan)?,
        source_delta,
        predecessor,
        lanes,
    })
}

fn lane_receipt(
    plan: &crate::workflow::Plan,
    entry: &crate::workflow::MatrixEntry,
    directive: &QualificationCacheDirective,
    admission: Option<&QualificationCacheAdmission>,
) -> TestResult<QualificationCacheLaneReceipt> {
    let layers = [
        QualificationCacheLayer::MbxObjects,
        QualificationCacheLayer::MbxBundle,
        QualificationCacheLayer::CargoSources,
        QualificationCacheLayer::MiseTools,
        QualificationCacheLayer::TofuProviders,
        QualificationCacheLayer::TaskResult,
    ]
    .into_iter()
    .enumerate()
    .map(|(index, layer)| layer_receipt(plan, entry, directive, admission, layer, index as u64))
    .collect::<TestResult<Vec<_>>>()?;
    let prior = admission
        .map(|value| prior_lane(value, &entry.matrix_key))
        .transpose()?;
    let phase = plan.qualification.as_ref().map(|context| context.phase);
    let is_delta = phase == Some(QualificationPhase::UsefulDelta);
    let is_warm = phase == Some(QualificationPhase::Warm);
    Ok(QualificationCacheLaneReceipt {
        matrix_key: entry.matrix_key.clone(),
        stack_id: entry.stack_id.clone(),
        task_id: entry.task_id.clone(),
        completed_task_ids: completed_tasks(entry),
        closure_digest: is_delta
            .then(|| digest_b3(b"closure-useful-delta"))
            .or_else(|| prior.as_ref().map(|lane| lane.closure_digest.clone()))
            .unwrap_or_else(|| digest_b3(b"closure-cold")),
        useful_state_digest: is_delta
            .then(|| digest_b3(b"useful-delta"))
            .or_else(|| is_warm.then(|| digest_b3(b"useful-warm")))
            .or_else(|| prior.as_ref().map(|lane| lane.useful_state_digest.clone()))
            .unwrap_or_else(|| digest_b3(b"useful-cold")),
        layers,
    })
}

fn layer_receipt(
    plan: &crate::workflow::Plan,
    entry: &crate::workflow::MatrixEntry,
    directive: &QualificationCacheDirective,
    admission: Option<&QualificationCacheAdmission>,
    layer: QualificationCacheLayer,
    index: u64,
) -> TestResult<QualificationCacheLayerReceipt> {
    let planned = directive
        .layer(&entry.matrix_key, layer)
        .ok_or_else(|| std::io::Error::other("planned layer missing"))?;
    if !planned.active {
        return Ok(disabled_layer(planned));
    }
    let identity = runtime_identity(&planned.runtime);
    let keys = directive.bind_runtime(plan, admission, &entry.matrix_key, layer, &identity)?;
    let bound_restore = keys
        .restore
        .as_ref()
        .ok_or_else(|| std::io::Error::other("restore keys missing"))?;
    let prior = admission
        .map(|value| value.layer_receipt(&entry.matrix_key, layer))
        .transpose()?;
    let phase = plan.qualification.as_ref().map(|context| context.phase);
    let is_changed_cache_layer = layer == QualificationCacheLayer::CargoSources
        && matches!(
            phase,
            Some(QualificationPhase::Warm | QualificationPhase::UsefulDelta)
        );
    let state = if is_changed_cache_layer {
        digest_b3(format!("cargo-source-state-after-{phase:?}").as_bytes())
    } else {
        prior
            .as_ref()
            .and_then(|receipt| receipt.state_digest.clone())
            .unwrap_or_else(|| digest_b3(format!("cold-state-{layer:?}").as_bytes()))
    };
    let (restore, save) = observations(
        plan,
        &keys,
        bound_restore.requested_key.as_str(),
        bound_restore.expected_cache.clone(),
        &state,
        index,
    )?;
    Ok(QualificationCacheLayerReceipt {
        layer,
        active: true,
        identity_digest: planned.identity_digest.clone(),
        runtime_identity: Some(identity),
        state_digest: Some(state),
        restore,
        save,
    })
}

fn observations(
    plan: &crate::workflow::Plan,
    keys: &crate::workflow::BoundQualificationCacheKeys,
    requested_key: &str,
    expected_cache: Option<QualificationCacheBackendEntry>,
    state: &str,
    index: u64,
) -> TestResult<(QualificationCacheRestore, QualificationCacheSave)> {
    let restore = match expected_cache {
        Some(entry) => QualificationCacheRestore {
            requested_key: Some(requested_key.to_owned()),
            matched_key: Some(requested_key.to_owned()),
            matched_cache: QualificationCacheBackendObservation::Found(entry),
            result: QualificationCacheRestoreResult::Hit,
        },
        None => QualificationCacheRestore {
            requested_key: Some(requested_key.to_owned()),
            matched_key: None,
            matched_cache: QualificationCacheBackendObservation::Absent,
            result: QualificationCacheRestoreResult::Miss,
        },
    };
    let (action, requested_save, before, after) = match keys.save_key_for_state(state)? {
        Some(key) => save_observed(plan, key, index)?,
        None if keys.save_key.is_some() => (
            QualificationCacheSaveActionResult::NotRequired,
            None,
            QualificationCacheBackendObservation::NotQueried,
            QualificationCacheBackendObservation::NotQueried,
        ),
        None => (
            QualificationCacheSaveActionResult::Disabled,
            None,
            QualificationCacheBackendObservation::NotQueried,
            QualificationCacheBackendObservation::NotQueried,
        ),
    };
    Ok((
        restore,
        QualificationCacheSave {
            requested_key: requested_save,
            action,
            before,
            after,
        },
    ))
}

fn save_observed(
    plan: &crate::workflow::Plan,
    key: &str,
    index: u64,
) -> TestResult<(
    QualificationCacheSaveActionResult,
    Option<String>,
    QualificationCacheBackendObservation,
    QualificationCacheBackendObservation,
)> {
    let context = dispatch(plan)?;
    let entry = QualificationCacheBackendEntry {
        id: context.run_id * 10 + index + 1,
        key: key.to_owned(),
        git_ref: context.git_ref.clone(),
        size_bytes: 4_096,
    };
    Ok((
        QualificationCacheSaveActionResult::Succeeded,
        Some(key.to_owned()),
        QualificationCacheBackendObservation::Absent,
        QualificationCacheBackendObservation::Found(entry),
    ))
}

fn disabled_layer(planned: &QualificationCacheLayerDirective) -> QualificationCacheLayerReceipt {
    QualificationCacheLayerReceipt {
        layer: planned.layer,
        active: false,
        identity_digest: planned.identity_digest.clone(),
        runtime_identity: None,
        state_digest: None,
        restore: QualificationCacheRestore {
            requested_key: None,
            matched_key: None,
            matched_cache: QualificationCacheBackendObservation::NotQueried,
            result: QualificationCacheRestoreResult::Disabled,
        },
        save: QualificationCacheSave {
            requested_key: None,
            action: QualificationCacheSaveActionResult::Disabled,
            before: QualificationCacheBackendObservation::NotQueried,
            after: QualificationCacheBackendObservation::NotQueried,
        },
    }
}

fn prior_lane<'a>(
    admission: &'a QualificationCacheAdmission,
    matrix_key: &str,
) -> Result<&'a QualificationCacheLaneReceipt, crate::ContractError> {
    admission
        .receipt()
        .lanes
        .iter()
        .find(|lane| lane.matrix_key == matrix_key)
        .ok_or_else(|| crate::ContractError::identity("test", "prior_lane_missing"))
}

fn completed_tasks(entry: &crate::workflow::MatrixEntry) -> Vec<String> {
    let mut tasks = entry
        .execute_task_ids
        .tasks
        .values()
        .flat_map(|task| match task {
            crate::workflow::ExecuteTaskRef::Single(value) => vec![value.clone()],
            crate::workflow::ExecuteTaskRef::Shards(values) => values.clone(),
        })
        .collect::<Vec<_>>();
    tasks.sort();
    tasks
}

fn receipt_link(previous: &BuiltNode) -> TestResult<QualificationCacheReceiptLink> {
    Ok(QualificationCacheReceiptLink {
        run: previous.receipt.run,
        receipt_digest: digest_b3(&canonical_json_bytes(&previous.receipt)?),
        artifact_id: previous.artifact.id,
        artifact_digest: previous.artifact.digest.clone(),
    })
}

pub(super) fn admission_for(
    node: &BuiltNode,
    source_delta: Option<&QualificationSourceDelta>,
) -> TestResult<QualificationCacheAdmission> {
    admission_from_value(&node.value, source_delta)
}

pub(super) fn admission_from_value(
    node: &serde_json::Value,
    source_delta: Option<&QualificationSourceDelta>,
) -> TestResult<QualificationCacheAdmission> {
    let bytes = serde_json::to_vec(&serde_json::json!({
        "predecessor": node,
        "source_delta": source_delta,
    }))?;
    Ok(QualificationCacheAdmission::parse_bounded(&bytes)?)
}

pub(super) fn useful_delta(base: &str, source: &str) -> TestResult<QualificationSourceDelta> {
    let changed_paths = vec!["crates/demo/src/lib.rs".to_owned()];
    Ok(QualificationSourceDelta {
        base_source_sha: base.to_owned(),
        source_sha: source.to_owned(),
        diff_digest: digest_b3(&canonical_json_bytes(&changed_paths)?),
        changed_paths,
        base_is_ancestor: true,
    })
}
