use super::super::github::{GitHubApi, GitRefResponse, TagObjectResponse};
use super::super::identity::is_git_sha;
use super::{Authorization, Intent, Tag, canonical, sha256};
use crate::{OrchestratorError, internal::internal};
use velnor_actions_contract::PackageReleasePlan;

pub(super) fn prepare_tag_object(
    api: &GitHubApi,
    plan: &PackageReleasePlan,
    authorization: &Authorization,
) -> Result<Tag, OrchestratorError> {
    let name = format!("v{}", plan.base_version);
    let message = authorization_message(authorization)?;
    let object = api.create_tag_object(&name, &message, &authorization.context.source_sha)?;
    validate_created_object(&object, &name, &message, &authorization.context.source_sha)?;
    Ok(Tag {
        name,
        object_sha: object.sha,
        object_type: "tag".to_owned(),
        target_sha: authorization.context.source_sha.clone(),
        message,
    })
}

pub(super) fn create_or_resume_tag(
    api: &GitHubApi,
    plan: &PackageReleasePlan,
    retained: &Intent,
) -> Result<Tag, OrchestratorError> {
    let expected = &retained.tag;
    let object: TagObjectResponse =
        api.get_json(&api.route(&format!("git/tags/{}", expected.object_sha)))?;
    validate_created_object(
        &object,
        &expected.name,
        &expected.message,
        &expected.target_sha,
    )?;
    if object.sha != expected.object_sha {
        return Err(internal("stable_tag_retained_object_changed"));
    }
    if api.tag_identity(&expected.name)?.is_some() {
        let observed = observe_tag(api, plan)?;
        if observed != *expected {
            return Err(internal("stable_tag_preexisting_unowned"));
        }
        return Ok(observed);
    }
    api.post_fields(
        &api.route("git/refs"),
        &[
            ("ref", format!("refs/tags/{}", expected.name)),
            ("sha", expected.object_sha.clone()),
        ],
    )?;
    let observed = observe_tag(api, plan)?;
    if observed != *expected {
        return Err(internal("stable_tag_created_object_changed"));
    }
    Ok(observed)
}

pub(super) fn authorization_message(
    authorization: &Authorization,
) -> Result<String, OrchestratorError> {
    Ok(format!(
        "Velnor stable tag authorization\nauthorization_sha256={}\n",
        sha256(&canonical(authorization)?)
    ))
}

pub(super) fn validate_created_object(
    object: &TagObjectResponse,
    name: &str,
    message: &str,
    source: &str,
) -> Result<(), OrchestratorError> {
    if object.tag != name
        || object.message != message
        || object.object.object_type != "commit"
        || object.object.sha != source
        || !is_git_sha(&object.sha)
    {
        return Err(internal("stable_tag_created_object_invalid"));
    }
    Ok(())
}

pub(super) fn observe_tag(
    api: &GitHubApi,
    plan: &PackageReleasePlan,
) -> Result<Tag, OrchestratorError> {
    let name = format!("v{}", plan.base_version);
    let reference: GitRefResponse = api.get_json(&api.route(&format!("git/ref/tags/{name}")))?;
    if reference.object.object_type != "tag" || !is_git_sha(&reference.object.sha) {
        return Err(internal("stable_tag_object_invalid"));
    }
    let object: TagObjectResponse =
        api.get_json(&api.route(&format!("git/tags/{}", reference.object.sha)))?;
    if object.sha != reference.object.sha
        || object.tag != name
        || object.object.object_type != "commit"
    {
        return Err(internal("stable_tag_object_invalid"));
    }
    let target_sha = object.object.sha;
    let final_ref: GitRefResponse = api.get_json(&api.route(&format!("git/ref/tags/{name}")))?;
    if final_ref.ref_name != reference.ref_name
        || final_ref.object.sha != reference.object.sha
        || final_ref.object.object_type != reference.object.object_type
        || !is_git_sha(&target_sha)
    {
        return Err(internal("stable_tag_changed_during_observation"));
    }
    if reference.ref_name != format!("refs/tags/{name}")
        || !is_git_sha(&reference.object.sha)
        || reference.object.object_type != "tag"
    {
        return Err(internal("stable_tag_object_invalid"));
    }
    Ok(Tag {
        name,
        object_sha: reference.object.sha,
        object_type: reference.object.object_type,
        target_sha,
        message: object.message,
    })
}
