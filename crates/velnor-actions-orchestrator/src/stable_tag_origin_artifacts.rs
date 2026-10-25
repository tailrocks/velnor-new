use std::collections::BTreeMap;
use velnor_actions_contract::parse_strict_json;

use super::super::{artifact_zip, github::GitHubApi};
use super::{
    ArtifactRef, Authorization, Intent, Origin, canonical, ensure_directory, fixed_path,
    read_metadata, sha256,
};
use crate::{OrchestratorError, internal::internal};

const LIMIT: usize = 4 * 1024 * 1024;

pub(super) fn intent(
    api: &GitHubApi,
    expected: &Authorization,
    reference: &ArtifactRef,
) -> Result<Intent, OrchestratorError> {
    let files = acquire(api, owner(expected), reference, "intent", &["intent.json"])?;
    let value: Intent = decode(&files["intent.json"])?;
    validate_intent(&value, expected)?;
    Ok(value)
}

pub(super) fn validate_intent(
    value: &Intent,
    expected: &Authorization,
) -> Result<(), OrchestratorError> {
    if value.schema != "velnor.stable-tag-intent.v1"
        || value.authorization != *expected
        || value.tag.name != format!("v{}", expected.context.version)
        || value.tag.object_type != "tag"
        || !super::super::identity::is_git_sha(&value.tag.object_sha)
        || value.tag.target_sha != expected.context.source_sha
        || value.tag.message != super::tag::authorization_message(expected)?
    {
        return Err(internal("stable_intent_artifact_substitution"));
    }
    Ok(())
}

pub(super) fn origin(
    api: &GitHubApi,
    expected: &Authorization,
    reference: &ArtifactRef,
) -> Result<Origin, OrchestratorError> {
    let files = acquire(api, owner(expected), reference, "origin", &["origin.json"])?;
    let value: Origin = decode(&files["origin.json"])?;
    validate_origin(&value, expected)?;
    Ok(value)
}

pub(super) fn validate_origin(
    value: &Origin,
    expected: &Authorization,
) -> Result<(), OrchestratorError> {
    if value.schema != "velnor.stable-tag-origin.v1" || value.completion_context != expected.context
    {
        return Err(internal("stable_origin_artifact_substitution"));
    }
    super::recovery::require_origin(&value.authorization, expected)?;
    if value.tag.message != super::tag::authorization_message(&value.authorization)? {
        return Err(internal("stable_origin_annotation_substitution"));
    }
    Ok(())
}

pub(super) fn require_intent_object(
    origin: &Origin,
    intent: &Intent,
) -> Result<(), OrchestratorError> {
    if origin.authorization != intent.authorization || origin.tag != intent.tag {
        return Err(internal("stable_tag_intent_object_substitution"));
    }
    Ok(())
}

#[derive(Clone, Copy)]
pub(super) struct ArtifactOwner<'a> {
    pub source: super::super::run_identity::RunSource<'a>,
    pub run_id: u64,
    pub attempt: u32,
}

fn owner(expected: &Authorization) -> ArtifactOwner<'_> {
    ArtifactOwner {
        source: super::super::run_identity::RunSource {
            repository_id: expected.context.repository_id,
            sha: &expected.context.source_sha,
            branch: expected
                .context
                .source_ref
                .trim_start_matches("refs/heads/"),
        },
        run_id: expected.context.run_id,
        attempt: expected.context.run_attempt,
    }
}

pub(super) fn read_prior_intent(
    api: &GitHubApi,
    owner: ArtifactOwner<'_>,
    reference: &ArtifactRef,
) -> Result<Intent, OrchestratorError> {
    let files = acquire(api, owner, reference, "intent", &["intent.json"])?;
    decode(&files["intent.json"])
}

fn acquire(
    api: &GitHubApi,
    owner: ArtifactOwner<'_>,
    reference: &ArtifactRef,
    kind: &str,
    names: &[&str],
) -> Result<BTreeMap<String, Vec<u8>>, OrchestratorError> {
    let name = format!(
        "velnor-stable-tag-{kind}-r{}-a{}",
        owner.run_id, owner.attempt
    );
    let artifacts = api.workflow_artifacts(owner.run_id)?;
    let matches: Vec<_> = artifacts
        .iter()
        .filter(|artifact| artifact.name == name)
        .collect();
    if matches.len() != 1 {
        return Err(internal("stable_tag_artifact_ambiguous"));
    }
    let artifact = matches[0];
    if artifact.id != reference.id
        || artifact.expired
        || artifact.size_in_bytes == 0
        || artifact.size_in_bytes > LIMIT as u64
        || artifact.digest.as_deref() != Some(format!("sha256:{}", reference.sha256).as_str())
        || artifact.workflow_run.as_ref().is_none_or(|run| {
            run.id != owner.run_id
                || run.repository_id != owner.source.repository_id
                || run.head_sha != owner.source.sha
                || run.head_branch != owner.source.branch
        })
    {
        return Err(internal("stable_tag_artifact_identity"));
    }
    let bytes = api.download_artifact_archive_bounded(reference.id, LIMIT)?;
    if bytes.len() as u64 != artifact.size_in_bytes || sha256(&bytes) != reference.sha256 {
        return Err(internal("stable_tag_artifact_archive_digest"));
    }
    let parent = fixed_path("temporary")?
        .parent()
        .ok_or_else(|| internal("stable_tag_temp"))?
        .to_owned();
    ensure_directory(&parent)?;
    let directory = tempfile::tempdir_in(parent).map_err(|_| internal("stable_tag_temp"))?;
    let entries = names
        .iter()
        .map(|name| artifact_zip::ExpectedZipEntry {
            name: (*name).to_owned(),
            size: None,
            sha256: None,
            max_bytes: 1024 * 1024,
            unix_permissions: None,
        })
        .collect::<Vec<_>>();
    artifact_zip::extract_in_scope(&bytes, directory.path(), &entries)?;
    names
        .iter()
        .map(|name| {
            Ok((
                (*name).to_owned(),
                read_metadata(&directory.path().join(name))?,
            ))
        })
        .collect()
}

pub(super) fn decode<T: serde::de::DeserializeOwned + serde::Serialize>(
    bytes: &[u8],
) -> Result<T, OrchestratorError> {
    let text = std::str::from_utf8(bytes).map_err(|_| internal("stable_tag_json_utf8"))?;
    let value = parse_strict_json(text).map_err(|_| internal("stable_tag_json_structure"))?;
    let decoded: T =
        serde_json::from_value(value).map_err(|_| internal("stable_tag_json_schema"))?;
    if canonical(&decoded)? != bytes {
        return Err(internal("stable_tag_json_canonical"));
    }
    Ok(decoded)
}
