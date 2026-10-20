//! Load and validate the plan-bound documents used by runtime binding.

use velnor_actions_contract::run_key_for_ci;
use velnor_actions_contract::workflow::{
    QualificationCacheDirective, QualificationRuntimeIdentity,
};

use super::files::{
    RunDirectory, parse_canonical, parse_layer, read_admission, read_plan,
    validate_dispatch_context,
};
use super::qualification_cache_runtime_identity_filename;
use super::{
    MAX_RUNTIME_IDENTITY_JSON_BYTES, QUALIFICATION_CACHE_DIRECTIVE_FILENAME,
    QUALIFICATION_CACHE_DIRECTIVE_MAX_BYTES, QualificationCacheBindingRequest,
};
use crate::OrchestratorError;
use crate::internal::internal;

pub(super) struct BindingDocuments {
    pub(super) plan: velnor_actions_contract::Plan,
    pub(super) admission: Option<velnor_actions_contract::workflow::QualificationCacheAdmission>,
    pub(super) directive: QualificationCacheDirective,
    pub(super) evidence: Option<QualificationRuntimeIdentity>,
}

pub(super) fn read_binding_documents(
    request: &QualificationCacheBindingRequest<'_>,
) -> Result<BindingDocuments, OrchestratorError> {
    let run_key = run_key_for_ci(request.run_id, u64::from(request.run_attempt));
    let run_dir = RunDirectory::open(request.runner_temp, &run_key)?;
    let plan = read_plan(&run_dir, &run_key)?;
    validate_dispatch_context(&plan, request)?;
    let admission = read_admission(&run_dir, &plan)?;
    let directive_bytes = run_dir.read_owned_file(
        QUALIFICATION_CACHE_DIRECTIVE_FILENAME,
        QUALIFICATION_CACHE_DIRECTIVE_MAX_BYTES,
        None,
        "qualification_cache_file",
    )?;
    let directive_text = std::str::from_utf8(&directive_bytes)
        .map_err(|_| internal("qualification_cache_directive_encoding"))?;
    let directive = parse_canonical::<QualificationCacheDirective>(
        directive_text,
        QUALIFICATION_CACHE_DIRECTIVE_MAX_BYTES,
    )?;
    let active = directive
        .owner_layer(request.owner_id, parse_layer(request.layer)?)
        .ok_or_else(|| internal("qualification_cache_owner_layer_missing"))?
        .active;
    let before_name =
        qualification_cache_runtime_identity_filename(request.owner_id, request.layer)?;
    let after_name =
        super::qualification_cache_runtime_after_import_filename(request.owner_id, request.layer)?;
    let before_path = request
        .runner_temp
        .join("velnor")
        .join(&run_key)
        .join(&before_name);
    let after_path = request
        .runner_temp
        .join("velnor")
        .join(&run_key)
        .join(&after_name);
    if !active {
        if request.runtime_identity_path != before_path
            || run_dir.file_exists(&before_name)?
            || run_dir.file_exists(&after_name)?
        {
            return Err(internal("qualification_cache_disabled_layer_runtime_file"));
        }
        return Ok(BindingDocuments {
            plan,
            admission,
            directive,
            evidence: None,
        });
    }
    let (filename, compare_before) = if request.runtime_identity_path == before_path {
        (before_name.as_str(), false)
    } else if request.runtime_identity_path == after_path {
        (after_name.as_str(), true)
    } else {
        return Err(internal("qualification_cache_runtime_identity_path"));
    };
    let evidence = read_identity(&run_dir, filename)?;
    if compare_before && read_identity(&run_dir, &before_name)? != evidence {
        return Err(internal("qualification_cache_runtime_changed_after_import"));
    }
    Ok(BindingDocuments {
        plan,
        admission,
        directive,
        evidence: Some(evidence),
    })
}

fn read_identity(
    run_dir: &RunDirectory,
    filename: &str,
) -> Result<QualificationRuntimeIdentity, OrchestratorError> {
    let bytes = run_dir.read_owned_file(
        filename,
        MAX_RUNTIME_IDENTITY_JSON_BYTES,
        Some(0o600),
        "qualification_cache_runtime_identity",
    )?;
    let text = std::str::from_utf8(&bytes)
        .map_err(|_| internal("qualification_cache_runtime_identity_encoding"))?;
    parse_canonical::<QualificationRuntimeIdentity>(text, MAX_RUNTIME_IDENTITY_JSON_BYTES)
}
