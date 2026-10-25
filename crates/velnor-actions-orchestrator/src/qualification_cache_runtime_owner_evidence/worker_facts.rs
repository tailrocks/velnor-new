//! Bounded worker-facts persistence for physical cache owners.

use std::path::{Path, PathBuf};

use velnor_actions_contract::canonical_json_bytes;
use velnor_actions_contract::workflow::{
    QualificationCacheLayer, QualificationCacheRestoreResult, QualificationCacheSaveActionResult,
};

use crate::OrchestratorError;
use crate::internal::internal;
use crate::qualification_cache_runtime::{
    QualificationCacheBindingRequest, bind_qualification_cache_runtime,
    qualification_cache_runtime_identity_filename,
};
use crate::qualification_cache_worker_facts::{
    QualificationCacheWorkerLayerFacts, QualificationCacheWorkerOwnerFacts,
};

use super::{
    MAX_OWNER_EVIDENCE_BYTES, MAX_OWNER_LAYER_EVIDENCE_BYTES, absolute_run_path,
    layer_receipt_relative_path, owner_directive, owner_receipt_relative_path,
    owner_static_documents,
};

const LAYERS: [QualificationCacheLayer; 5] = [
    QualificationCacheLayer::MbxObjects,
    QualificationCacheLayer::CargoSources,
    QualificationCacheLayer::MiseTools,
    QualificationCacheLayer::TofuProviders,
    QualificationCacheLayer::TaskResult,
];

/// Validate and persist one layer of worker-produced cache facts.
///
/// # Errors
/// Returns an error if facts do not match the validated plan, runtime, keys, or path.
pub fn write_qualification_cache_worker_layer_facts(
    request: &QualificationCacheBindingRequest<'_>,
    facts: &QualificationCacheWorkerLayerFacts,
) -> Result<String, OrchestratorError> {
    let (run_dir, run_key, directive) = owner_static_documents(request)?;
    let owner = owner_directive(&directive, request.owner_id)?;
    let layer = super::parse_layer(request.layer)?;
    let expected = owner
        .layers
        .iter()
        .find(|entry| entry.layer == layer)
        .ok_or_else(|| internal("qualification_cache_owner_layer_missing"))?;
    validate_layer_facts(request, facts, owner, expected)?;
    let bytes = canonical_json_bytes(facts).map_err(crate::internal::internal_contract)?;
    if bytes.len() > MAX_OWNER_LAYER_EVIDENCE_BYTES {
        return Err(internal("qualification_cache_worker_layer_oversize"));
    }
    let relative = layer_receipt_relative_path(request.owner_id, layer)?;
    run_dir.write_owned_relative_file(
        &relative,
        &bytes,
        "qualification_cache_worker_layer_facts",
    )?;
    Ok(absolute_run_path(request.runner_temp, &run_key, &relative))
}

/// Combine five ordered worker layer files into the existing owner artifact path.
///
/// # Errors
/// Returns an error if any file is missing, noncanonical, or bound to another owner.
pub fn aggregate_qualification_cache_worker_owner_facts(
    request: &QualificationCacheBindingRequest<'_>,
    layer_paths: &[PathBuf],
) -> Result<String, OrchestratorError> {
    let (run_dir, run_key, directive) = owner_static_documents(request)?;
    let owner = owner_directive(&directive, request.owner_id)?;
    if layer_paths.len() != LAYERS.len() {
        return Err(internal("qualification_cache_worker_layer_set_mismatch"));
    }
    let mut layers = Vec::with_capacity(LAYERS.len());
    for (path, layer) in layer_paths.iter().zip(LAYERS) {
        let relative = layer_receipt_relative_path(request.owner_id, layer)?;
        let expected_path = request
            .runner_temp
            .join("velnor")
            .join(&run_key)
            .join(&relative);
        if path != &expected_path {
            return Err(internal("qualification_cache_worker_layer_path_mismatch"));
        }
        let bytes = run_dir.read_owned_relative_file(
            &relative,
            MAX_OWNER_LAYER_EVIDENCE_BYTES,
            Some(0o600),
            "qualification_cache_worker_layer_facts",
        )?;
        let layer_facts = QualificationCacheWorkerLayerFacts::parse_canonical(&bytes)?;
        let expected = owner
            .layers
            .iter()
            .find(|entry| entry.layer == layer)
            .ok_or_else(|| internal("qualification_cache_owner_layer_missing"))?;
        if layer_facts.owner_id != owner.owner_id
            || layer_facts.platform_id != owner.platform_id
            || layer_facts.member_digest != owner.member_digest
            || layer_facts.identity_digest != expected.identity_digest
            || layer_facts.active != expected.active
            || layer_facts.layer != layer
        {
            return Err(internal("qualification_cache_worker_layer_mismatch"));
        }
        let runtime_path = request.runner_temp.join("velnor").join(&run_key).join(
            qualification_cache_runtime_identity_filename(request.owner_id, layer.as_str())?,
        );
        let layer_request = request_for_layer(request, layer.as_str(), &runtime_path);
        validate_layer_facts(&layer_request, &layer_facts, owner, expected)?;
        layers.push(layer_facts);
    }
    let facts = QualificationCacheWorkerOwnerFacts {
        schema: QualificationCacheWorkerOwnerFacts::SCHEMA,
        kind: QualificationCacheWorkerOwnerFacts::KIND.to_owned(),
        owner_id: owner.owner_id.clone(),
        platform_id: owner.platform_id.clone(),
        member_digest: owner.member_digest.clone(),
        members: owner.members.clone(),
        layers,
    };
    facts.validate()?;
    let bytes = canonical_json_bytes(&facts).map_err(crate::internal::internal_contract)?;
    if bytes.len() > MAX_OWNER_EVIDENCE_BYTES {
        return Err(internal("qualification_cache_worker_owner_oversize"));
    }
    let relative = owner_receipt_relative_path(request.owner_id)?;
    run_dir.write_owned_relative_file(
        &relative,
        &bytes,
        "qualification_cache_worker_owner_facts",
    )?;
    Ok(absolute_run_path(request.runner_temp, &run_key, &relative))
}

fn request_for_layer<'a>(
    request: &QualificationCacheBindingRequest<'a>,
    layer: &'a str,
    runtime_identity_path: &'a Path,
) -> QualificationCacheBindingRequest<'a> {
    QualificationCacheBindingRequest {
        runner_temp: request.runner_temp,
        run_id: request.run_id,
        run_attempt: request.run_attempt,
        event_name: request.event_name,
        repository: request.repository,
        git_ref: request.git_ref,
        ref_protected: request.ref_protected,
        workflow_ref: request.workflow_ref,
        workflow_sha: request.workflow_sha,
        source_sha: request.source_sha,
        runner_os: request.runner_os,
        runner_arch: request.runner_arch,
        image_os: request.image_os,
        owner_id: request.owner_id,
        layer,
        runtime_identity_path,
    }
}

fn validate_layer_facts(
    request: &QualificationCacheBindingRequest<'_>,
    facts: &QualificationCacheWorkerLayerFacts,
    owner: &velnor_actions_contract::workflow::QualificationCacheOwnerDirective,
    expected: &velnor_actions_contract::workflow::QualificationCacheLayerDirective,
) -> Result<(), OrchestratorError> {
    if facts.owner_id != owner.owner_id
        || facts.platform_id != owner.platform_id
        || facts.member_digest != owner.member_digest
        || facts.layer != expected.layer
        || facts.active != expected.active
        || facts.identity_digest != expected.identity_digest
    {
        return Err(internal("qualification_cache_worker_layer_binding"));
    }
    if expected.active {
        let before = super::owner_documents(request)?;
        if before.before_runtime != facts.runtime_identity
            || before.after_runtime != facts.runtime_identity
        {
            return Err(internal("qualification_cache_worker_runtime_mismatch"));
        }
        let binding = bind_qualification_cache_runtime(request)?;
        validate_active_facts(facts, &binding)?;
    } else if facts.runtime_identity.is_some()
        || facts.state_digest.is_some()
        || facts.restore.result != QualificationCacheRestoreResult::Disabled
        || facts.restore.requested_key.is_some()
        || facts.restore.matched_key.is_some()
        || facts.save.action != QualificationCacheSaveActionResult::Disabled
        || facts.save.requested_key.is_some()
    {
        return Err(internal("qualification_cache_worker_disabled_access"));
    }
    facts.validate_shape()
}

fn validate_active_facts(
    facts: &QualificationCacheWorkerLayerFacts,
    binding: &super::QualificationCacheBindingOutputs,
) -> Result<(), OrchestratorError> {
    if facts.restore.requested_key != binding.restore_key {
        return Err(internal("qualification_cache_worker_restore_key"));
    }
    let restore_valid = match binding.expected_cache.as_ref() {
        Some(_) => {
            facts.restore.result == QualificationCacheRestoreResult::Hit
                && facts.restore.matched_key == binding.restore_key
        }
        None if binding.restore_key.is_some() => {
            facts.restore.result == QualificationCacheRestoreResult::Miss
                && facts.restore.matched_key.is_none()
        }
        None => facts.restore.result == QualificationCacheRestoreResult::Disabled,
    };
    if !restore_valid {
        return Err(internal("qualification_cache_worker_restore_result"));
    }
    match binding.save_key.as_deref() {
        None if facts.save.action == QualificationCacheSaveActionResult::Disabled
            && facts.save.requested_key.is_none() => {}
        Some(_)
            if facts.save.action == QualificationCacheSaveActionResult::NotRequired
                && facts.save.requested_key.is_none()
                && binding.expected_prior_state_digest.as_deref()
                    == facts.state_digest.as_deref() => {}
        Some(key)
            if matches!(
                facts.save.action,
                QualificationCacheSaveActionResult::Succeeded
                    | QualificationCacheSaveActionResult::Skipped
                    | QualificationCacheSaveActionResult::Failed
            ) && facts.save.requested_key.as_deref() == Some(key) => {}
        _ => return Err(internal("qualification_cache_worker_save_result")),
    }
    Ok(())
}
