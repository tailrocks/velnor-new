//! Retained intent and observed tag admission for manual stable production.
//!
//! These records authenticate the authorized operation and exact resulting tag.
//! They do not claim GitHub exposes a trustworthy tag creator identity.

use serde::{Deserialize, Serialize};
use std::{env, path::PathBuf};
use velnor_actions_contract::{
    PackageReleasePlan, canonical_json_bytes, package_release::PackageReleaseChannel,
    preview_production::ReleaseProductionContext,
};

use super::{
    github::{GitHubApi, GitRefResponse, WorkflowRunResponse, sha256},
    run_identity::{RunSource, run_matches},
};
use crate::{OrchestratorError, internal::internal};

#[path = "stable_tag_origin_admission.rs"]
mod admission;
#[path = "stable_tag_origin_artifacts.rs"]
mod artifacts;
#[path = "stable_tag_origin_io.rs"]
mod io;
#[path = "stable_tag_origin_recovery.rs"]
mod recovery;
#[path = "stable_tag_origin_retained.rs"]
mod retained;
#[path = "stable_tag_origin_tag.rs"]
mod tag;
pub use admission::admit_stable_tag_origin_from_env;
pub(crate) use admission::{admitted_policy_plan, admitted_production_context};
use io::{ensure_directory, read_metadata, write_new};
pub(super) use retained::verify_signed_origin;
use tag::{create_or_resume_tag, observe_tag};

#[path = "stable_tag_origin_selector.rs"]
mod selector;
pub use selector::{SELECT_STABLE_RELEASE_SOURCE_OP, select_stable_release_source_from_env};

/// Write pre-tag authority before the qualified metadata operation.
pub const WRITE_STABLE_TAG_INTENT_OP: &str = "write-stable-tag-intent-v1";
/// Bind the actual observed tag to its retained pre-tag authority.
pub const WRITE_STABLE_TAG_ORIGIN_OP: &str = "write-stable-tag-origin-v1";
/// Authenticate retained tag evidence before any producer checkout.
pub const ADMIT_STABLE_TAG_ORIGIN_OP: &str = "admit-stable-tag-origin-v1";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Authorization {
    schema: String,
    repository: String,
    policy_sha256: String,
    plan_sha256: String,
    version_package: String,
    context: ReleaseProductionContext,
    tag_was_absent: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Intent {
    schema: String,
    authorization: Authorization,
    tag: Tag,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ArtifactRef {
    id: u64,
    sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Tag {
    name: String,
    object_sha: String,
    object_type: String,
    target_sha: String,
    message: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Origin {
    schema: String,
    intent: ArtifactRef,
    authorization: Authorization,
    completion_context: ReleaseProductionContext,
    tag: Tag,
}

/// Persist authenticated manual-source authority while the stable tag is absent.
/// # Errors
/// Rejects an existing tag, invalid workflow/CI, or noncanonical source authority.
pub fn write_stable_tag_intent_from_env() -> Result<(), OrchestratorError> {
    super::artifact_zip::within_operation(write_intent)
}

fn write_intent() -> Result<(), OrchestratorError> {
    let plan = plan()?;
    require_writer_role(WRITE_STABLE_TAG_INTENT_OP)?;
    let intent = current_authorization(&plan)?;
    let api = GitHubApi::new(plan.repository.clone());
    if let Some((original, reference)) = recovery::original_intent(&api, &intent)? {
        authenticate(&api, &plan, &original.authorization.context, false)?;
        authenticate_current(&api, &plan, &intent.context, false)?;
        return recovery::outputs(Some(&reference));
    }
    authenticate_current(&api, &plan, &intent.context, true)?;
    if api
        .tag_identity(&format!("v{}", plan.base_version))?
        .is_some()
    {
        return Err(internal(
            "stable_tag_preexisting_without_intent_rerun_original",
        ));
    }
    let tag = tag::prepare_tag_object(&api, &plan, &intent)?;
    let retained = Intent {
        schema: "velnor.stable-tag-intent.v1".to_owned(),
        authorization: intent,
        tag,
    };
    write_new("intent.json", &canonical(&retained)?)?;
    recovery::outputs(None)
}

/// Persist the actual tag, preserving its original pre-tag intent artifact.
/// # Errors
/// Rejects altered intent, a different source, or missing tag.
pub fn write_stable_tag_origin_from_env() -> Result<(), OrchestratorError> {
    super::artifact_zip::within_operation(write_origin)
}

fn write_origin() -> Result<(), OrchestratorError> {
    let plan = plan()?;
    require_writer_role(WRITE_STABLE_TAG_ORIGIN_OP)?;
    let intent = current_authorization(&plan)?;
    let api = GitHubApi::new(plan.repository.clone());
    authenticate_current(&api, &plan, &intent.context, false)?;
    let reference = artifact_ref("VELNOR_STABLE_TAG_INTENT_ARTIFACT")?;
    let retained = recovery::selected_intent(&api, &intent, &reference)?;
    authenticate(&api, &plan, &retained.authorization.context, false)?;
    authenticate_current(&api, &plan, &intent.context, false)?;
    let tag = create_or_resume_tag(&api, &plan, &retained)?;
    if tag.target_sha != intent.context.source_sha {
        return Err(internal("stable_tag_target_mismatch"));
    }
    let origin = Origin {
        schema: "velnor.stable-tag-origin.v1".to_owned(),
        intent: reference,
        authorization: retained.authorization,
        completion_context: intent.context,
        tag,
    };
    write_new("origin.json", &canonical(&origin)?)
}

fn current_authorization(plan: &PackageReleasePlan) -> Result<Authorization, OrchestratorError> {
    authorization_for_context(plan, current_context(plan)?)
}

fn current_context(
    plan: &PackageReleasePlan,
) -> Result<ReleaseProductionContext, OrchestratorError> {
    let context = ReleaseProductionContext {
        repository_id: positive("GITHUB_REPOSITORY_ID")?,
        head_repository_id: positive("GITHUB_REPOSITORY_ID")?,
        source_sha: required("GITHUB_SHA")?,
        source_ref: required("GITHUB_REF")?,
        workflow_authority_sha: required("GITHUB_WORKFLOW_SHA")?,
        run_id: positive("GITHUB_RUN_ID")?,
        run_attempt: attempt("GITHUB_RUN_ATTEMPT")?,
        ci_run_id: positive("VELNOR_RELEASE_CI_RUN_ID")?,
        ci_run_attempt: attempt("VELNOR_RELEASE_CI_RUN_ATTEMPT")?,
        commit_count: 0,
        version: plan.base_version.clone(),
    };
    if required("GITHUB_EVENT_NAME")? != "workflow_dispatch"
        || required("GITHUB_REPOSITORY")? != plan.repository
        || required("GITHUB_WORKFLOW_REF")?
            != format!(
                "{}/{}@refs/heads/{}",
                plan.repository,
                plan.channel.workflow_path(),
                plan.default_branch
            )
    {
        return Err(internal("stable_tag_writer_role"));
    }
    velnor_actions_contract::preview_production::validate_release_production_context(
        plan, &context,
    )
    .map_err(|_| internal("stable_tag_context"))?;
    Ok(context)
}

fn authorization_for_context(
    plan: &PackageReleasePlan,
    context: ReleaseProductionContext,
) -> Result<Authorization, OrchestratorError> {
    velnor_actions_contract::preview_production::validate_release_production_context(
        plan, &context,
    )
    .map_err(|_| internal("stable_tag_context"))?;
    let (policy, resolved) =
        crate::release_policy_admission::resolve_policy_from_env(&context.source_sha)?;
    if resolved != *plan {
        return Err(internal("stable_tag_resolved_plan_changed"));
    }
    let version_package = policy.version.version_package.clone();
    Ok(Authorization {
        schema: "velnor.stable-tag-authorization.v1".to_owned(),
        repository: plan.repository.clone(),
        policy_sha256: sha256(
            &policy
                .canonical_bytes()
                .map_err(|_| internal("stable_tag_policy"))?,
        ),
        plan_sha256: sha256(
            &plan
                .canonical_bytes()
                .map_err(|_| internal("stable_tag_plan"))?,
        ),
        version_package,
        context,
        tag_was_absent: true,
    })
}

fn authenticate_current(
    api: &GitHubApi,
    plan: &PackageReleasePlan,
    context: &ReleaseProductionContext,
    latest_ci: bool,
) -> Result<(), OrchestratorError> {
    authenticate(api, plan, context, latest_ci)?;
    let current: WorkflowRunResponse =
        api.get_json(&api.route(&format!("actions/runs/{}", context.run_id)))?;
    if current.id != Some(context.run_id)
        || current.run_attempt != Some(context.run_attempt)
        || current.head_sha.as_deref() != Some(context.source_sha.as_str())
    {
        return Err(internal("stable_tag_stale_current_attempt"));
    }
    api.verify_stable_tag_environment(&plan.default_branch)
}

fn authenticate(
    api: &GitHubApi,
    plan: &PackageReleasePlan,
    context: &ReleaseProductionContext,
    latest: bool,
) -> Result<(), OrchestratorError> {
    api.verify_repository(context.repository_id)?;
    api.verify_stable_tag_environment(&plan.default_branch)?;
    let source = RunSource {
        repository_id: context.repository_id,
        sha: &context.source_sha,
        branch: &plan.default_branch,
    };
    let workflow =
        api.admitted_workflow(plan.channel.workflow_path(), plan.channel.workflow_name())?;
    let run = api.workflow_run(context.run_id, context.run_attempt)?;
    if !run_matches(
        &run,
        &workflow,
        context.run_id,
        context.run_attempt,
        source,
        "workflow_dispatch",
    ) || !matches!(run.status.as_deref(), Some("in_progress" | "completed"))
    {
        return Err(internal("stable_tag_workflow_identity"));
    }
    api.verify_ci_identity(context.ci_run_id, context.ci_run_attempt, source, latest)?;
    if latest {
        let current: GitRefResponse = api.get_json(&api.route(&format!(
            "git/ref/heads/{}",
            super::github::encode_segment(&plan.default_branch)
        )))?;
        if current.object.object_type != "commit" || current.object.sha != context.source_sha {
            return Err(internal("stable_tag_source_advanced_before_intent"));
        }
    }
    Ok(())
}

fn plan() -> Result<PackageReleasePlan, OrchestratorError> {
    let (_, plan) =
        crate::release_policy_admission::resolve_policy_from_env(&required("GITHUB_SHA")?)?;
    if plan.channel != PackageReleaseChannel::Stable {
        return Err(internal("stable_tag_wrong_channel"));
    }
    Ok(plan)
}

fn fixed_path(name: &str) -> Result<PathBuf, OrchestratorError> {
    let root = PathBuf::from(required("RUNNER_TEMP")?);
    if !root.is_absolute() {
        return Err(internal("stable_tag_runner_temp"));
    }
    Ok(root.join("velnor/stable-tag").join(name))
}

fn canonical(value: &impl Serialize) -> Result<Vec<u8>, OrchestratorError> {
    canonical_json_bytes(value).map_err(|_| internal("stable_tag_json"))
}
fn required(name: &str) -> Result<String, OrchestratorError> {
    env::var(name)
        .ok()
        .filter(|v| !v.is_empty())
        .ok_or_else(|| internal("stable_tag_environment"))
}
fn positive(name: &str) -> Result<u64, OrchestratorError> {
    let value = required(name)?;
    value
        .parse::<u64>()
        .ok()
        .filter(|n| *n > 0 && n.to_string() == value)
        .ok_or_else(|| internal("stable_tag_positive_identity"))
}
fn attempt(name: &str) -> Result<u32, OrchestratorError> {
    u32::try_from(positive(name)?).map_err(|_| internal("stable_tag_attempt"))
}
fn artifact_ref(prefix: &str) -> Result<ArtifactRef, OrchestratorError> {
    let digest = required(&format!("{prefix}_DIGEST"))?;
    let digest = digest.strip_prefix("sha256:").unwrap_or(&digest).to_owned();
    if digest.len() != 64
        || !digest
            .bytes()
            .all(|b| b.is_ascii_digit() || matches!(b, b'a'..=b'f'))
    {
        return Err(internal("stable_tag_archive_digest"));
    }
    Ok(ArtifactRef {
        id: positive(&format!("{prefix}_ID"))?,
        sha256: digest,
    })
}

fn require_writer_role(expected_operation: &str) -> Result<(), OrchestratorError> {
    if required("GITHUB_JOB")? != "stable-tag"
        || required("VELNOR_INTERNAL_OP")? != expected_operation
    {
        return Err(internal("stable_tag_writer_role"));
    }
    Ok(())
}

#[cfg(test)]
#[path = "stable_tag_origin_tests.rs"]
mod tests;
