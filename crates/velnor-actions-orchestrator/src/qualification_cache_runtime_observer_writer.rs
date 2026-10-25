//! Validate the owner directive, measure the host and persist one identity file.

use velnor_actions_contract::workflow::{
    QualificationCacheAdmission, QualificationCacheDirective, QualificationRuntimeIdentity,
    QualificationRuntimeIdentityRequirements, QualificationRuntimeImageIdentity,
    QualificationRuntimePlatform,
};
use velnor_actions_contract::{Plan, RunsOn, run_key_for_ci};

use crate::OrchestratorError;
use crate::internal::internal;

use super::files::{
    RunDirectory, parse_canonical, parse_layer, read_admission, read_guest_image_projection,
    read_plan, validate_dispatch_context, validate_runtime_environment,
    verified_scale_set_image_identity,
};
use super::{
    MAX_RUNTIME_IDENTITY_JSON_BYTES, QUALIFICATION_CACHE_DIRECTIVE_FILENAME,
    QUALIFICATION_CACHE_DIRECTIVE_MAX_BYTES, QualificationCacheBindingRequest,
    qualification_cache_runtime_identity_filename,
};

struct OwnerDocuments {
    plan: Plan,
    admission: Option<QualificationCacheAdmission>,
    directive: QualificationCacheDirective,
    run_key: String,
    run_dir: RunDirectory,
}

pub(super) fn observe_and_write<E>(
    request: &QualificationCacheBindingRequest<'_>,
    get: E,
) -> Result<String, OrchestratorError>
where
    E: FnMut(&str) -> Option<String>,
{
    let mut documents = load_owner_documents(request)?;
    let layer = parse_layer(request.layer)?;
    let directive = documents
        .directive
        .owner_layer(request.owner_id, layer)
        .ok_or_else(|| internal("qualification_cache_owner_layer_missing"))?;
    if !directive.active {
        return inactive_identity_path(request, &documents);
    }
    let evidence = observe_owner(request, &documents, get)?;
    validate_owner_binding(request, &documents, &evidence)?;
    let filename = qualification_cache_runtime_identity_filename(request.owner_id, request.layer)?;
    persist_identity(request, &mut documents, &evidence, &filename)
}

fn inactive_identity_path(
    request: &QualificationCacheBindingRequest<'_>,
    documents: &OwnerDocuments,
) -> Result<String, OrchestratorError> {
    let before_name =
        qualification_cache_runtime_identity_filename(request.owner_id, request.layer)?;
    let after_name =
        super::qualification_cache_runtime_after_import_filename(request.owner_id, request.layer)?;
    if documents.run_dir.file_exists(&before_name)? || documents.run_dir.file_exists(&after_name)? {
        return Err(internal("qualification_cache_disabled_layer_runtime_file"));
    }
    Ok(request
        .runner_temp
        .join("velnor")
        .join(&documents.run_key)
        .join(before_name)
        .display()
        .to_string())
}

pub(super) fn observe_after_import<E>(
    request: &QualificationCacheBindingRequest<'_>,
    get: E,
) -> Result<(String, bool), OrchestratorError>
where
    E: FnMut(&str) -> Option<String>,
{
    let mut documents = load_owner_documents(request)?;
    let before_name =
        qualification_cache_runtime_identity_filename(request.owner_id, request.layer)?;
    let before_bytes = documents.run_dir.read_owned_file(
        &before_name,
        MAX_RUNTIME_IDENTITY_JSON_BYTES,
        Some(0o600),
        "qualification_cache_runtime_identity",
    )?;
    let before_text = std::str::from_utf8(&before_bytes)
        .map_err(|_| internal("qualification_cache_runtime_identity_encoding"))?;
    let before = parse_canonical::<QualificationRuntimeIdentity>(
        before_text,
        MAX_RUNTIME_IDENTITY_JSON_BYTES,
    )?;
    let after = observe_owner(request, &documents, get)?;
    validate_owner_binding(request, &documents, &after)?;
    let matches = before == after;
    if !matches {
        return Err(internal("qualification_cache_runtime_changed_after_import"));
    }
    let filename =
        super::qualification_cache_runtime_after_import_filename(request.owner_id, request.layer)?;
    let path = persist_identity(request, &mut documents, &after, &filename)?;
    Ok((path, matches))
}

fn load_owner_documents(
    request: &QualificationCacheBindingRequest<'_>,
) -> Result<OwnerDocuments, OrchestratorError> {
    super::validate_runner_temp(request.runner_temp)?;
    velnor_actions_contract::workflow::validate_job_id(request.owner_id)
        .map_err(crate::internal::internal_contract)?;
    let run_key = run_key_for_ci(request.run_id, u64::from(request.run_attempt));
    let run_dir = RunDirectory::open(request.runner_temp, &run_key)?;
    let plan = read_plan(&run_dir, &run_key)?;
    validate_dispatch_context(&plan, request)?;
    require_owner_entry(&plan, request.owner_id)?;
    let admission = read_admission(&run_dir, &plan)?;
    let directive = read_directive(&run_dir)?;
    directive
        .owner_layer(request.owner_id, parse_layer(request.layer)?)
        .ok_or_else(|| internal("qualification_cache_owner_layer_missing"))?;
    Ok(OwnerDocuments {
        plan,
        admission,
        directive,
        run_key,
        run_dir,
    })
}

fn require_owner_entry(plan: &Plan, owner_id: &str) -> Result<(), OrchestratorError> {
    if plan
        .matrix
        .include
        .iter()
        .any(|entry| entry.job_id == owner_id)
    {
        Ok(())
    } else {
        Err(internal("qualification_cache_owner_missing"))
    }
}

fn read_directive(
    run_dir: &RunDirectory,
) -> Result<QualificationCacheDirective, OrchestratorError> {
    let bytes = run_dir.read_owned_file(
        QUALIFICATION_CACHE_DIRECTIVE_FILENAME,
        QUALIFICATION_CACHE_DIRECTIVE_MAX_BYTES,
        None,
        "qualification_cache_file",
    )?;
    let text = std::str::from_utf8(&bytes)
        .map_err(|_| internal("qualification_cache_directive_encoding"))?;
    parse_canonical(text, QUALIFICATION_CACHE_DIRECTIVE_MAX_BYTES)
}

fn observe_owner<E>(
    request: &QualificationCacheBindingRequest<'_>,
    documents: &OwnerDocuments,
    mut get: E,
) -> Result<QualificationRuntimeIdentity, OrchestratorError>
where
    E: FnMut(&str) -> Option<String>,
{
    validate_process_platform(request)?;
    let entry = owner_entry(&documents.plan, request.owner_id)?;
    let layer = parse_layer(request.layer)?;
    let owner = owner_record(&documents.directive, request.owner_id)?;
    let requirements = owner_requirements(owner, layer)?;
    let scale_set_image = scale_set_image_identity(request, entry, requirements)?;
    let platform = runtime_platform(request, requirements, scale_set_image.clone())?;
    let (mise_data, rustup_home) = validate_tool_homes(request, &mut get)?;
    let evidence = super::observation::observe(platform, requirements, &mise_data, &rustup_home)?;
    validate_runtime_environment(
        &evidence,
        &documents.plan,
        entry,
        request,
        scale_set_image.as_ref(),
    )?;
    Ok(evidence)
}

fn owner_entry<'a>(
    plan: &'a Plan,
    owner_id: &str,
) -> Result<&'a velnor_actions_contract::workflow::MatrixEntry, OrchestratorError> {
    plan.matrix
        .include
        .iter()
        .find(|entry| entry.job_id == owner_id)
        .ok_or_else(|| internal("qualification_cache_owner_missing"))
}

fn owner_record<'a>(
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

fn owner_requirements(
    owner: &velnor_actions_contract::workflow::QualificationCacheOwnerDirective,
    layer: velnor_actions_contract::workflow::QualificationCacheLayer,
) -> Result<&QualificationRuntimeIdentityRequirements, OrchestratorError> {
    owner
        .layers
        .iter()
        .find(|record| record.layer == layer && record.runtime.is_some())
        .or_else(|| {
            owner
                .layers
                .iter()
                .find(|record| record.active && record.runtime.is_some())
        })
        .and_then(|record| record.runtime.as_ref())
        .ok_or_else(|| internal("qualification_cache_runtime_requirements_missing"))
}

fn scale_set_image_identity(
    request: &QualificationCacheBindingRequest<'_>,
    entry: &velnor_actions_contract::workflow::MatrixEntry,
    requirements: &QualificationRuntimeIdentityRequirements,
) -> Result<Option<QualificationRuntimeImageIdentity>, OrchestratorError> {
    match RunsOn::parse(&entry.planned_platform.runs_on)
        .map_err(crate::internal::internal_contract)?
    {
        RunsOn::Hosted(_) => Ok(None),
        RunsOn::ScaleSet(selector) => {
            if request.image_os.is_some() {
                return Err(internal(
                    "qualification_cache_runtime_image_source_mismatch",
                ));
            }
            let source = requirements
                .scale_set_image_source_sha
                .as_deref()
                .ok_or_else(|| internal("qualification_guest_image_source_authority_missing"))?;
            let projection = read_guest_image_projection()?;
            Ok(Some(verified_scale_set_image_identity(
                &projection,
                source,
                &selector,
            )?))
        }
    }
}

fn runtime_platform(
    request: &QualificationCacheBindingRequest<'_>,
    requirements: &QualificationRuntimeIdentityRequirements,
    scale_set_image: Option<QualificationRuntimeImageIdentity>,
) -> Result<QualificationRuntimePlatform, OrchestratorError> {
    let image_identity = match scale_set_image {
        Some(identity) => identity,
        None => QualificationRuntimeImageIdentity::Hosted {
            image_os: request
                .image_os
                .filter(|image| !image.is_empty())
                .ok_or_else(|| internal("qualification_cache_runtime_image_missing"))?
                .to_owned(),
        },
    };
    Ok(QualificationRuntimePlatform {
        os: request.runner_os.to_owned(),
        arch: request.runner_arch.to_owned(),
        runner_label: requirements.runner_label.clone(),
        runs_on: requirements.runs_on.clone(),
        image_identity,
        target: requirements.target.clone(),
    })
}

fn validate_tool_homes<E>(
    request: &QualificationCacheBindingRequest<'_>,
    get: &mut E,
) -> Result<(std::path::PathBuf, std::path::PathBuf), OrchestratorError>
where
    E: FnMut(&str) -> Option<String>,
{
    let mise_data = request.runner_temp.join("velnor/mise");
    let rustup_home = request.runner_temp.join("velnor/rustup");
    for (name, expected) in [
        ("MISE_DATA_DIR", &mise_data),
        ("RUSTUP_HOME", &rustup_home),
        ("MISE_RUSTUP_HOME", &rustup_home),
    ] {
        if get(name).as_deref() != expected.to_str() {
            return Err(internal("qualification_runtime_tool_home_mismatch"));
        }
    }
    Ok((mise_data, rustup_home))
}

fn validate_owner_binding(
    request: &QualificationCacheBindingRequest<'_>,
    documents: &OwnerDocuments,
    evidence: &QualificationRuntimeIdentity,
) -> Result<(), OrchestratorError> {
    let owner = owner_record(&documents.directive, request.owner_id)?;
    let active_layer = owner
        .layers
        .iter()
        .find(|layer| layer.active && layer.runtime.is_some())
        .map(|layer| layer.layer)
        .ok_or_else(|| internal("qualification_cache_runtime_requirements_missing"))?;
    for layer in [active_layer, parse_layer(request.layer)?] {
        documents
            .directive
            .bind_runtime_owner(
                &documents.plan,
                documents.admission.as_ref(),
                request.owner_id,
                layer,
                evidence,
            )
            .map_err(crate::internal::internal_contract)?;
    }
    Ok(())
}

fn persist_identity(
    request: &QualificationCacheBindingRequest<'_>,
    documents: &mut OwnerDocuments,
    evidence: &QualificationRuntimeIdentity,
    filename: &str,
) -> Result<String, OrchestratorError> {
    let canonical = velnor_actions_contract::canonical_json_str(evidence)
        .map_err(crate::internal::internal_contract)?;
    if canonical.len() > MAX_RUNTIME_IDENTITY_JSON_BYTES {
        return Err(internal("qualification_cache_runtime_identity_oversize"));
    }
    let expected = request
        .runner_temp
        .join("velnor")
        .join(&documents.run_key)
        .join(filename);
    if request.runtime_identity_path != expected {
        return Err(internal("qualification_cache_runtime_identity_path"));
    }
    documents.run_dir.write_owned_file(
        filename,
        canonical.as_bytes(),
        "qualification_cache_runtime_identity",
    )?;
    Ok(expected.display().to_string())
}

fn validate_process_platform(
    request: &QualificationCacheBindingRequest<'_>,
) -> Result<(), OrchestratorError> {
    let os = match std::env::consts::OS {
        "linux" => "Linux",
        "macos" => "macOS",
        "windows" => "Windows",
        _ => return Err(internal("qualification_runtime_host_unsupported")),
    };
    let arch = match std::env::consts::ARCH {
        "x86_64" => "X64",
        "aarch64" => "ARM64",
        "x86" => "X86",
        _ => return Err(internal("qualification_runtime_host_unsupported")),
    };
    if request.runner_os != os || request.runner_arch != arch {
        return Err(internal("qualification_cache_runtime_environment_mismatch"));
    }
    Ok(())
}
