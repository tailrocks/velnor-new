//! Owner-layer evidence files stored under the pinned qualification run directory.

use std::path::{Path, PathBuf};

use serde::{Serialize, de::DeserializeOwned};
use velnor_actions_contract::workflow::{
    QualificationCacheDirective, QualificationCacheLayer, QualificationCacheLayerDirective,
    QualificationCacheLayerReceipt, QualificationCacheOwnerDirective,
    QualificationCacheOwnerReceipt, QualificationRuntimeIdentity,
};
use velnor_actions_contract::{canonical_json_bytes, run_key_for_ci};

use crate::OrchestratorError;
use crate::internal::internal;

use super::files::{
    RunDirectory, parse_canonical, parse_layer, read_admission, read_plan,
    validate_dispatch_context,
};
use super::{
    QUALIFICATION_CACHE_DIRECTIVE_FILENAME, QUALIFICATION_CACHE_DIRECTIVE_MAX_BYTES,
    QualificationCacheBindingOutputs, QualificationCacheBindingRequest,
    bind_qualification_cache_runtime, qualification_cache_runtime_after_import_filename,
    qualification_cache_runtime_identity_filename,
};

const OWNER_EVIDENCE_ROOT: &str = "qualification-cache/owners";
const LAYER_EVIDENCE_ROOT: &str = "qualification-cache/owner-layers";
const MAX_OWNER_LAYER_EVIDENCE_BYTES: usize = 65_536;
const MAX_OWNER_EVIDENCE_BYTES: usize = 262_144;
const MAX_RUNTIME_IDENTITY_BYTES: usize = 65_536;

/// Plan-bound static and observed inputs for one owner layer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QualificationCacheOwnerLayerContext {
    /// Exact generated job owner and all its planned members.
    pub owner: QualificationCacheOwnerDirective,
    /// One layer's validated static directive.
    pub layer: QualificationCacheLayerDirective,
    /// Runtime identity re-observed after all restores, if the layer is active.
    pub runtime_identity: Option<QualificationRuntimeIdentity>,
}

struct OwnerDocuments {
    run_dir: RunDirectory,
    run_key: String,
    directive: QualificationCacheDirective,
    before_runtime: Option<QualificationRuntimeIdentity>,
    after_runtime: Option<QualificationRuntimeIdentity>,
}

/// Read one owner layer and require equal pre-import and post-import runtime facts.
///
/// # Errors
///
/// Returns an error if the owner directive or runtime records do not match.
pub fn qualification_cache_owner_layer_context(
    request: &QualificationCacheBindingRequest<'_>,
) -> Result<QualificationCacheOwnerLayerContext, OrchestratorError> {
    let documents = owner_documents(request)?;
    let layer = parse_layer(request.layer)?;
    let owner = owner_directive(&documents.directive, request.owner_id)?;
    let layer_directive = owner
        .layers
        .iter()
        .find(|record| record.layer == layer)
        .ok_or_else(|| internal("qualification_cache_owner_layer_missing"))?;
    let runtime_identity = if layer_directive.active {
        let before = documents
            .before_runtime
            .ok_or_else(|| internal("qualification_cache_runtime_missing"))?;
        let after = documents
            .after_runtime
            .ok_or_else(|| internal("qualification_cache_runtime_after_import_missing"))?;
        if before != after {
            return Err(internal("qualification_cache_runtime_changed_after_import"));
        }
        Some(after)
    } else {
        if documents.before_runtime.is_some() || documents.after_runtime.is_some() {
            return Err(internal("qualification_cache_disabled_layer_has_runtime"));
        }
        None
    };
    Ok(QualificationCacheOwnerLayerContext {
        owner: owner.clone(),
        layer: layer_directive.clone(),
        runtime_identity,
    })
}

/// Write one validated owner-layer observation to the current run directory.
///
/// # Errors
///
/// Returns an error if the run, owner, layer, identity, or evidence file does
/// not agree with the staged plan and directive.
pub fn write_qualification_cache_owner_layer_evidence(
    request: &QualificationCacheBindingRequest<'_>,
    receipt: &QualificationCacheLayerReceipt,
) -> Result<String, OrchestratorError> {
    let documents = owner_documents(request)?;
    let layer = parse_layer(request.layer)?;
    let owner = owner_directive(&documents.directive, request.owner_id)?;
    let expected = owner
        .layers
        .iter()
        .find(|record| record.layer == layer)
        .ok_or_else(|| internal("qualification_cache_owner_layer_missing"))?;
    let binding = if expected.active {
        Some(bind_qualification_cache_runtime(request)?)
    } else {
        None
    };
    validate_layer_receipt(
        receipt,
        owner,
        expected,
        documents.before_runtime.as_ref(),
        documents.after_runtime.as_ref(),
        binding.as_ref(),
        request.git_ref,
    )?;
    let bytes = canonical_json_bytes(receipt).map_err(crate::internal::internal_contract)?;
    if bytes.len() > MAX_OWNER_LAYER_EVIDENCE_BYTES {
        return Err(internal("qualification_cache_owner_layer_oversize"));
    }
    let relative = layer_receipt_relative_path(request.owner_id, layer)?;
    documents.run_dir.write_owned_relative_file(
        &relative,
        &bytes,
        "qualification_cache_owner_layer",
    )?;
    Ok(absolute_run_path(
        request.runner_temp,
        &documents.run_key,
        &relative,
    ))
}

/// Combine the five ordered layer records for one generated job owner.
///
/// # Errors
///
/// Returns an error if any layer file is missing, duplicated, noncanonical,
/// or bound to another owner or directive.
pub fn aggregate_qualification_cache_owner_evidence(
    request: &QualificationCacheBindingRequest<'_>,
    layer_paths: &[PathBuf],
) -> Result<String, OrchestratorError> {
    let (run_dir, run_key, directive) = owner_static_documents(request)?;
    let owner = owner_directive(&directive, request.owner_id)?;
    let expected_layers = [
        QualificationCacheLayer::MbxObjects,
        QualificationCacheLayer::CargoSources,
        QualificationCacheLayer::MiseTools,
        QualificationCacheLayer::TofuProviders,
        QualificationCacheLayer::TaskResult,
    ];
    if layer_paths.len() != expected_layers.len() {
        return Err(internal("qualification_cache_owner_layer_set_mismatch"));
    }
    let mut layers = Vec::with_capacity(expected_layers.len());
    for (path, layer) in layer_paths.iter().zip(expected_layers) {
        let relative = layer_receipt_relative_path(request.owner_id, layer)?;
        let expected_path = request
            .runner_temp
            .join("velnor")
            .join(&run_key)
            .join(&relative);
        if path != &expected_path {
            return Err(internal("qualification_cache_owner_layer_path_mismatch"));
        }
        let bytes = run_dir.read_owned_relative_file(
            &relative,
            MAX_OWNER_LAYER_EVIDENCE_BYTES,
            Some(0o600),
            "qualification_cache_owner_layer",
        )?;
        let receipt: QualificationCacheLayerReceipt = parse_canonical_bytes(&bytes)?;
        let expected = owner
            .layers
            .iter()
            .find(|record| record.layer == layer)
            .ok_or_else(|| internal("qualification_cache_owner_layer_missing"))?;
        if receipt.owner_id != owner.owner_id
            || receipt.platform_id != owner.platform_id
            || receipt.member_digest != owner.member_digest
            || receipt.layer != layer
            || receipt.identity_digest != expected.identity_digest
            || receipt.active != expected.active
        {
            return Err(internal("qualification_cache_owner_layer_mismatch"));
        }
        layers.push(receipt);
    }
    let receipt = QualificationCacheOwnerReceipt {
        owner_id: owner.owner_id.clone(),
        platform_id: owner.platform_id.clone(),
        member_digest: owner.member_digest.clone(),
        members: owner.members.clone(),
        layers,
    };
    let bytes = canonical_json_bytes(&receipt).map_err(crate::internal::internal_contract)?;
    if bytes.len() > MAX_OWNER_EVIDENCE_BYTES {
        return Err(internal("qualification_cache_owner_oversize"));
    }
    let relative = owner_receipt_relative_path(request.owner_id)?;
    run_dir.write_owned_relative_file(&relative, &bytes, "qualification_cache_owner")?;
    Ok(absolute_run_path(request.runner_temp, &run_key, &relative))
}

/// Read one immutable owner record from a downloaded job artifact.
///
/// # Errors
///
/// Returns an error if the path or canonical JSON is invalid.
pub fn read_qualification_cache_owner_evidence(
    runner_temp: &Path,
    run_id: u64,
    run_attempt: u32,
    owner_id: &str,
    path: &Path,
) -> Result<QualificationCacheOwnerReceipt, OrchestratorError> {
    velnor_actions_contract::workflow::validate_job_id(owner_id)
        .map_err(crate::internal::internal_contract)?;
    let run_key = run_key_for_ci(run_id, u64::from(run_attempt));
    let relative = owner_receipt_relative_path(owner_id)?;
    let expected = runner_temp.join("velnor").join(&run_key).join(&relative);
    if path != expected {
        return Err(internal("qualification_cache_owner_path_mismatch"));
    }
    let run_dir = RunDirectory::open(runner_temp, &run_key)?;
    let bytes = run_dir.read_owned_relative_file(
        &relative,
        MAX_OWNER_EVIDENCE_BYTES,
        Some(0o600),
        "qualification_cache_owner",
    )?;
    parse_canonical_bytes(&bytes)
}

fn owner_documents(
    request: &QualificationCacheBindingRequest<'_>,
) -> Result<OwnerDocuments, OrchestratorError> {
    let (run_dir, run_key, directive) = owner_static_documents(request)?;
    let before_name =
        qualification_cache_runtime_identity_filename(request.owner_id, request.layer)?;
    let after_name =
        qualification_cache_runtime_after_import_filename(request.owner_id, request.layer)?;
    let before = read_optional_runtime(&run_dir, &before_name)?;
    let after = read_optional_runtime(&run_dir, &after_name)?;
    Ok(OwnerDocuments {
        run_dir,
        run_key,
        directive,
        before_runtime: before,
        after_runtime: after,
    })
}

fn owner_static_documents(
    request: &QualificationCacheBindingRequest<'_>,
) -> Result<(RunDirectory, String, QualificationCacheDirective), OrchestratorError> {
    let run_key = run_key_for_ci(request.run_id, u64::from(request.run_attempt));
    let run_dir = RunDirectory::open(request.runner_temp, &run_key)?;
    let plan = read_plan(&run_dir, &run_key)?;
    validate_dispatch_context(&plan, request)?;
    let _admission = read_admission(&run_dir, &plan)?;
    let bytes = run_dir.read_owned_file(
        QUALIFICATION_CACHE_DIRECTIVE_FILENAME,
        QUALIFICATION_CACHE_DIRECTIVE_MAX_BYTES,
        None,
        "qualification_cache_file",
    )?;
    let text = std::str::from_utf8(&bytes)
        .map_err(|_| internal("qualification_cache_directive_encoding"))?;
    let directive = parse_canonical::<QualificationCacheDirective>(
        text,
        QUALIFICATION_CACHE_DIRECTIVE_MAX_BYTES,
    )?;
    Ok((run_dir, run_key, directive))
}

fn read_optional_runtime(
    run_dir: &RunDirectory,
    filename: &str,
) -> Result<Option<QualificationRuntimeIdentity>, OrchestratorError> {
    if !run_dir.file_exists(filename)? {
        return Ok(None);
    }
    let bytes = run_dir.read_owned_file(
        filename,
        MAX_RUNTIME_IDENTITY_BYTES,
        Some(0o600),
        "qualification_cache_runtime_identity",
    )?;
    parse_canonical_bytes(&bytes).map(Some)
}

#[path = "qualification_cache_runtime_owner_validation.rs"]
mod validation;
use validation::validate_layer_receipt;

fn owner_directive<'a>(
    directive: &'a QualificationCacheDirective,
    owner_id: &str,
) -> Result<
    &'a velnor_actions_contract::workflow::QualificationCacheOwnerDirective,
    OrchestratorError,
> {
    directive
        .owners
        .iter()
        .find(|owner| owner.owner_id == owner_id)
        .ok_or_else(|| internal("qualification_cache_owner_missing"))
}

fn layer_receipt_relative_path(
    owner_id: &str,
    layer: QualificationCacheLayer,
) -> Result<String, OrchestratorError> {
    velnor_actions_contract::workflow::validate_job_id(owner_id)
        .map_err(crate::internal::internal_contract)?;
    Ok(format!(
        "{LAYER_EVIDENCE_ROOT}/{owner_id}-{}.json",
        layer.as_str()
    ))
}

fn owner_receipt_relative_path(owner_id: &str) -> Result<String, OrchestratorError> {
    velnor_actions_contract::workflow::validate_job_id(owner_id)
        .map_err(crate::internal::internal_contract)?;
    Ok(format!("{OWNER_EVIDENCE_ROOT}/{owner_id}.json"))
}

fn absolute_run_path(runner_temp: &Path, run_key: &str, relative: &str) -> String {
    runner_temp
        .join("velnor")
        .join(run_key)
        .join(relative)
        .display()
        .to_string()
}

fn parse_canonical_bytes<T: DeserializeOwned + Serialize>(
    bytes: &[u8],
) -> Result<T, OrchestratorError> {
    let text = std::str::from_utf8(bytes)
        .map_err(|_| internal("qualification_cache_evidence_encoding"))?;
    parse_canonical(text, MAX_OWNER_EVIDENCE_BYTES)
}
