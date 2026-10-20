use super::*;

pub(super) fn lane_receipt(
    plan: &crate::workflow::Plan,
    entry: &crate::workflow::MatrixEntry,
    directive: &QualificationCacheDirective,
    admission: Option<&QualificationCacheAdmission>,
) -> TestResult<QualificationCacheLaneReceipt> {
    let layers = [
        QualificationCacheLayer::MbxObjects,
        QualificationCacheLayer::CargoSources,
        QualificationCacheLayer::MiseTools,
        QualificationCacheLayer::TofuProviders,
        QualificationCacheLayer::TaskResult,
    ]
    .into_iter()
    .map(|layer| {
        let planned = directive
            .layer(&entry.matrix_key, layer)
            .ok_or_else(|| std::io::Error::other("The planned owner layer is missing."))?;
        Ok(QualificationCacheLaneLayerReceipt {
            layer,
            active: planned.active,
            identity_digest: planned.identity_digest.clone(),
            owner_id: planned.active.then(|| entry.job_id.clone()),
        })
    })
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

pub(super) fn owner_receipts(
    plan: &crate::workflow::Plan,
    directive: &QualificationCacheDirective,
    admission: Option<&QualificationCacheAdmission>,
) -> TestResult<Vec<QualificationCacheOwnerReceipt>> {
    directive
        .owners
        .iter()
        .map(|owner| {
            let entry = plan
                .matrix
                .include
                .iter()
                .find(|entry| entry.job_id == owner.owner_id)
                .ok_or_else(|| std::io::Error::other("The owner has no matrix entry."))?;
            let layers = owner
                .layers
                .iter()
                .enumerate()
                .map(|(index, planned)| {
                    layer_receipt(
                        plan,
                        entry,
                        directive,
                        admission,
                        planned.layer,
                        index as u64,
                    )
                })
                .collect::<TestResult<Vec<_>>>()?;
            Ok(QualificationCacheOwnerReceipt {
                owner_id: owner.owner_id.clone(),
                platform_id: owner.platform_id.clone(),
                member_digest: owner.member_digest.clone(),
                members: owner.members.clone(),
                layers,
            })
        })
        .collect()
}

fn layer_receipt(
    plan: &crate::workflow::Plan,
    entry: &crate::workflow::MatrixEntry,
    directive: &QualificationCacheDirective,
    admission: Option<&QualificationCacheAdmission>,
    layer: QualificationCacheLayer,
    index: u64,
) -> TestResult<QualificationCacheLayerReceipt> {
    let owner = directive
        .owners
        .iter()
        .find(|owner| owner.owner_id == entry.job_id)
        .ok_or_else(|| std::io::Error::other("The owner directive is missing."))?;
    let planned = directive
        .layer(&entry.matrix_key, layer)
        .ok_or_else(|| std::io::Error::other("The planned layer is missing."))?;
    if !planned.active {
        return Ok(disabled_layer(planned, owner));
    }
    let runtime = planned
        .runtime
        .as_ref()
        .ok_or_else(|| std::io::Error::other("The runtime requirements are missing."))?;
    let identity = runtime_identity(runtime);
    let keys = directive.bind_runtime_owner(plan, admission, &entry.job_id, layer, &identity)?;
    let bound_restore = keys
        .restore
        .as_ref()
        .ok_or_else(|| std::io::Error::other("The restore keys are missing."))?;
    let prior = admission
        .map(|value| value.layer_receipt(&entry.job_id, layer))
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
        owner_id: owner.owner_id.clone(),
        platform_id: owner.platform_id.clone(),
        member_digest: owner.member_digest.clone(),
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
        version: "a".repeat(64),
        size_bytes: 4_096,
    };
    Ok((
        QualificationCacheSaveActionResult::Succeeded,
        Some(key.to_owned()),
        QualificationCacheBackendObservation::Absent,
        QualificationCacheBackendObservation::Found(entry),
    ))
}

fn disabled_layer(
    planned: &QualificationCacheLayerDirective,
    owner: &QualificationCacheOwnerDirective,
) -> QualificationCacheLayerReceipt {
    QualificationCacheLayerReceipt {
        owner_id: owner.owner_id.clone(),
        platform_id: owner.platform_id.clone(),
        member_digest: owner.member_digest.clone(),
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

pub(super) fn receipt_link(previous: &BuiltNode) -> TestResult<QualificationCacheReceiptLink> {
    Ok(QualificationCacheReceiptLink {
        run: previous.receipt.run,
        receipt_digest: digest_b3(&canonical_json_bytes(&previous.receipt)?),
        artifact_id: previous.artifact.id,
        artifact_digest: previous.artifact.digest.clone(),
    })
}
