//! Re-derive the closed operation from runner context and complete source policy.

use crate::{
    OrchestratorError, internal::internal, retrieve_reports::staged_reads::read_staged_bytes_until,
};
use serde::Deserialize;
use std::{collections::BTreeMap, env, path::Path};
use velnor_actions_contract::{
    PackageReleasePlan,
    preview_identity::PreviewIdentity,
    preview_production::{
        PREVIEW_ASSEMBLE_JOB_ID, PREVIEW_PRODUCTION_SCHEMA_V1, PayloadArtifactRef,
        PreviewPayloadPair, PreviewProductionContext, PreviewProductionRequest,
        PreviewProductionStage, expected_preview_payload_pairs, preview_payload_job_id,
        preview_plan_sha256,
    },
};
use velnor_actions_mise::CheckDeadline;

pub(super) fn required_env(name: &str) -> Result<String, OrchestratorError> {
    env::var(name)
        .ok()
        .filter(|value| !value.is_empty())
        .ok_or_else(|| internal("preview_context_missing"))
}

pub(super) fn positive_env(name: &str) -> Result<u64, OrchestratorError> {
    positive(&required_env(name)?)
}

fn positive(value: &str) -> Result<u64, OrchestratorError> {
    let number = value
        .parse::<u64>()
        .ok()
        .filter(|value| *value > 0)
        .ok_or_else(|| internal("preview_context_number"))?;
    if number.to_string() != value {
        return Err(internal("preview_context_number"));
    }
    Ok(number)
}

pub(super) fn derive(
    root: &Path,
    plan: PackageReleasePlan,
    deadline: CheckDeadline,
) -> Result<PreviewProductionRequest, OrchestratorError> {
    let context = derive_context(root, &plan, deadline)?;
    from_context(plan, context)
}

pub(super) fn from_context(
    plan: PackageReleasePlan,
    context: PreviewProductionContext,
) -> Result<PreviewProductionRequest, OrchestratorError> {
    let stage = stage(&plan, &context)?;
    let request = PreviewProductionRequest {
        schema: PREVIEW_PRODUCTION_SCHEMA_V1.to_owned(),
        plan_sha256: preview_plan_sha256(&plan).map_err(|_| internal("preview_plan_digest"))?,
        plan,
        context,
        stage,
    };
    request
        .validate()
        .map_err(|_| internal("preview_request_invalid"))?;
    validate_role(&request, &required_env("GITHUB_JOB")?)?;
    Ok(request)
}

pub(super) fn derive_context(
    root: &Path,
    plan: &PackageReleasePlan,
    deadline: CheckDeadline,
) -> Result<PreviewProductionContext, OrchestratorError> {
    let context = workflow_context(plan, deadline)?;
    let count = crate::preview_producer::source_commit_count(root, &context.source_sha, deadline)?;
    if count != context.commit_count {
        return Err(internal("preview_eligibility_commit_count_mismatch"));
    }
    let identity = PreviewIdentity::derive(&plan.base_version, &context.source_sha, count)
        .map_err(|_| internal("preview_identity_invalid"))?;
    crate::preview_producer::validate_preview_source(root, &identity, deadline)?;
    Ok(context)
}

/// Runner claims are independently authenticated by the remote transport caller.
/// This source-free constructor does not claim local full-history verification.
pub(super) fn workflow_context(
    plan: &PackageReleasePlan,
    deadline: CheckDeadline,
) -> Result<PreviewProductionContext, OrchestratorError> {
    validate_workflow(plan)?;
    let source_sha = required_env("GITHUB_SHA")?;
    let count = positive_env("VELNOR_RELEASE_COMMIT_COUNT")?;
    let identity = PreviewIdentity::derive(&plan.base_version, &source_sha, count)
        .map_err(|_| internal("preview_identity_invalid"))?;
    let repository_id = positive_env("GITHUB_REPOSITORY_ID")?;
    let context = PreviewProductionContext {
        repository_id,
        head_repository_id: repository_id,
        source_sha,
        source_ref: required_env("GITHUB_REF")?,
        workflow_authority_sha: required_env("GITHUB_WORKFLOW_SHA")?,
        run_id: positive_env("GITHUB_RUN_ID")?,
        run_attempt: u32::try_from(positive_env("GITHUB_RUN_ATTEMPT")?)
            .map_err(|_| internal("preview_context_number"))?,
        ci_run_id: positive_env("VELNOR_RELEASE_CI_RUN_ID")?,
        ci_run_attempt: u32::try_from(positive_env("VELNOR_RELEASE_CI_RUN_ATTEMPT")?)
            .map_err(|_| internal("preview_context_number"))?,
        commit_count: count,
        version: identity.version,
    };
    velnor_actions_contract::preview_production::validate_preview_production_context(
        plan, &context,
    )
    .map_err(|_| internal("preview_context_invalid"))?;
    validate_event(plan, &context, deadline)?;
    Ok(context)
}

pub(super) fn validate_role(
    request: &PreviewProductionRequest,
    job: &str,
) -> Result<(), OrchestratorError> {
    let expected = match &request.stage {
        PreviewProductionStage::Payload { pair } => preview_payload_job_id(&request.plan, pair)
            .map_err(|_| internal("preview_payload_role"))?,
        PreviewProductionStage::Assemble { .. } => PREVIEW_ASSEMBLE_JOB_ID.to_owned(),
    };
    if job != expected {
        return Err(internal("preview_job_role_mismatch"));
    }
    Ok(())
}

fn stage(
    plan: &PackageReleasePlan,
    context: &PreviewProductionContext,
) -> Result<PreviewProductionStage, OrchestratorError> {
    match required_env("VELNOR_PREVIEW_STAGE")?.as_str() {
        "payload" => {
            if env::var_os("VELNOR_PREVIEW_NEEDS_JSON").is_some() {
                return Err(internal("preview_payload_unexpected_inputs"));
            }
            Ok(PreviewProductionStage::Payload {
                pair: PreviewPayloadPair {
                    product_id: required_env("VELNOR_PREVIEW_PRODUCT_ID")?,
                    target: required_env("VELNOR_PREVIEW_TARGET")?,
                },
            })
        }
        "assemble" => {
            if ["VELNOR_PREVIEW_PRODUCT_ID", "VELNOR_PREVIEW_TARGET"]
                .iter()
                .any(|key| env::var_os(key).is_some())
            {
                return Err(internal("preview_assembly_unexpected_selector"));
            }
            Ok(PreviewProductionStage::Assemble {
                inputs: parse_needs(&required_env("VELNOR_PREVIEW_NEEDS_JSON")?, plan, context)?,
            })
        }
        _ => Err(internal("preview_stage_unknown")),
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Need {
    result: String,
    outputs: BTreeMap<String, String>,
}

pub(super) fn parse_needs(
    text: &str,
    plan: &PackageReleasePlan,
    context: &PreviewProductionContext,
) -> Result<Vec<PayloadArtifactRef>, OrchestratorError> {
    let value =
        velnor_actions_contract::strict_json::parse_strict_json_bytes(text.as_bytes(), 1024 * 1024)
            .map_err(|_| internal("preview_needs_json"))?;
    let mut needs: BTreeMap<String, Need> =
        serde_json::from_value(value).map_err(|_| internal("preview_needs_shape"))?;
    let eligibility = needs
        .remove("preview-eligibility")
        .ok_or_else(|| internal("preview_needs_eligibility"))?;
    let expected = BTreeMap::from([
        ("source_sha".to_owned(), context.source_sha.clone()),
        (
            "workflow_authority_sha".to_owned(),
            context.workflow_authority_sha.clone(),
        ),
        ("ci_run_id".to_owned(), context.ci_run_id.to_string()),
        ("ci_attempt".to_owned(), context.ci_run_attempt.to_string()),
        ("commit_count".to_owned(), context.commit_count.to_string()),
    ]);
    if eligibility.result != "success" || eligibility.outputs != expected {
        return Err(internal("preview_needs_eligibility"));
    }
    let pairs = expected_preview_payload_pairs(plan).map_err(|_| internal("preview_needs_plan"))?;
    let mut inputs = Vec::with_capacity(pairs.len());
    for pair in pairs {
        let job =
            preview_payload_job_id(plan, &pair).map_err(|_| internal("preview_needs_plan"))?;
        let need = needs
            .remove(&job)
            .ok_or_else(|| internal("preview_needs_missing_leaf"))?;
        if need.result != "success" || need.outputs.len() != 2 {
            return Err(internal("preview_needs_leaf"));
        }
        inputs.push(PayloadArtifactRef {
            pair,
            artifact_id: positive(
                need.outputs
                    .get("artifact_id")
                    .ok_or_else(|| internal("preview_needs_leaf"))?,
            )?,
            sha256: need
                .outputs
                .get("artifact_digest")
                .ok_or_else(|| internal("preview_needs_leaf"))?
                .clone(),
        });
    }
    if !needs.is_empty() {
        return Err(internal("preview_needs_extra_job"));
    }
    Ok(inputs)
}

fn validate_workflow(plan: &PackageReleasePlan) -> Result<(), OrchestratorError> {
    if required_env("GITHUB_EVENT_NAME")? != "workflow_run"
        || !workflow_identity_matches(
            plan,
            &required_env("GITHUB_REPOSITORY")?,
            &required_env("GITHUB_REF")?,
            &required_env("GITHUB_SHA")?,
            &required_env("GITHUB_WORKFLOW_SHA")?,
            &required_env("GITHUB_WORKFLOW_REF")?,
        )
    {
        return Err(internal("preview_workflow_context_mismatch"));
    }
    Ok(())
}

pub(super) fn workflow_identity_matches(
    plan: &PackageReleasePlan,
    repository: &str,
    reference: &str,
    sha: &str,
    authority: &str,
    workflow: &str,
) -> bool {
    repository.eq_ignore_ascii_case(&plan.repository)
        && reference == format!("refs/heads/{}", plan.default_branch)
        && sha == authority
        && sha.len() == 40
        && sha
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        && workflow == format!("{repository}/{}@{reference}", plan.channel.workflow_path())
}

fn validate_event(
    plan: &PackageReleasePlan,
    context: &PreviewProductionContext,
    deadline: CheckDeadline,
) -> Result<(), OrchestratorError> {
    let bytes = read_staged_bytes_until(
        Path::new(&required_env("GITHUB_EVENT_PATH")?),
        4 * 1024 * 1024,
        || {
            deadline
                .remaining()
                .map(|_| ())
                .map_err(|_| "preview_deadline")
        },
    )
    .map_err(|_| internal("preview_event_read"))?;
    let event =
        velnor_actions_contract::strict_json::parse_strict_json_bytes(&bytes, 4 * 1024 * 1024)
            .map_err(|_| internal("preview_event_json"))?;
    let run = &event["workflow_run"];
    if event["action"].as_str() != Some("completed")
        || event["repository"]["id"].as_u64() != Some(context.repository_id)
        || event["repository"]["full_name"].as_str() != Some(plan.repository.as_str())
        || run["id"].as_u64() != Some(context.ci_run_id)
        || run["run_attempt"].as_u64() != Some(u64::from(context.ci_run_attempt))
        || run["head_repository"]["id"].as_u64() != Some(context.repository_id)
        || run["head_sha"].as_str() != Some(context.source_sha.as_str())
        || run["head_branch"].as_str() != Some(plan.default_branch.as_str())
        || run["event"].as_str() != Some("push")
        || run["status"].as_str() != Some("completed")
        || run["conclusion"].as_str() != Some("success")
        || run["path"].as_str() != Some(velnor_actions_contract::CI_WORKFLOW_PATH)
        || run["name"].as_str() != Some(velnor_actions_contract::WORKFLOW_DISPLAY_NAME)
    {
        return Err(internal("preview_event_identity"));
    }
    Ok(())
}

#[cfg(test)]
#[path = "preview_production_context_tests.rs"]
mod tests;
