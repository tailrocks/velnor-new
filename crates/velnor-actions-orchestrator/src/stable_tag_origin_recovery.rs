use super::super::github::GitHubApi;
use super::{ArtifactRef, Authorization, Intent, artifacts, required};
use crate::{OrchestratorError, internal::internal};
use std::io::Write as _;

pub(super) fn original_intent(
    api: &GitHubApi,
    current: &Authorization,
) -> Result<Option<(Intent, ArtifactRef)>, OrchestratorError> {
    let Some((attempt, reference)) =
        prior_reference(api, current.context.run_id, current.context.run_attempt)?
    else {
        return Ok(None);
    };
    let mut expected = current.clone();
    expected.context.run_attempt = attempt;
    let original = artifacts::intent(api, &expected, &reference)?;
    require_completion(&original.authorization, current)?;
    Ok(Some((original, reference)))
}

pub(super) fn prior_reference(
    api: &GitHubApi,
    run_id: u64,
    current_attempt: u32,
) -> Result<Option<(u32, ArtifactRef)>, OrchestratorError> {
    let prefix = format!("velnor-stable-tag-intent-r{run_id}-a");
    let mut matches = Vec::new();
    for artifact in api.workflow_artifacts(run_id)? {
        let Some(suffix) = artifact.name.strip_prefix(&prefix) else {
            continue;
        };
        let attempt = suffix
            .parse::<u32>()
            .ok()
            .filter(|attempt| *attempt > 0 && attempt.to_string() == suffix)
            .ok_or_else(|| internal("stable_original_intent_name"))?;
        if attempt >= current_attempt {
            continue;
        }
        let digest = artifact
            .digest
            .as_deref()
            .and_then(|value| value.strip_prefix("sha256:"))
            .filter(|value| valid_digest(value))
            .ok_or_else(|| internal("stable_original_intent_digest"))?;
        matches.push((
            attempt,
            ArtifactRef {
                id: artifact.id,
                sha256: digest.to_owned(),
            },
        ));
    }
    match matches.as_slice() {
        [] => Ok(None),
        [(attempt, reference)] => Ok(Some((*attempt, reference.clone()))),
        _ => Err(internal("stable_original_intent_ambiguous")),
    }
}

pub(super) fn selected_intent(
    api: &GitHubApi,
    current: &Authorization,
    reference: &ArtifactRef,
) -> Result<Intent, OrchestratorError> {
    if let Some((original, original_ref)) = original_intent(api, current)? {
        if original_ref != *reference {
            return Err(internal("stable_original_intent_pointer"));
        }
        return Ok(original);
    }
    artifacts::intent(api, current, reference)
}

pub(super) fn require_completion(
    original: &Authorization,
    current: &Authorization,
) -> Result<(), OrchestratorError> {
    if original.context.run_attempt >= current.context.run_attempt {
        return Err(internal("stable_original_intent_attempt"));
    }
    let mut expected = original.clone();
    expected.context.run_attempt = current.context.run_attempt;
    if expected != *current {
        return Err(internal("stable_original_intent_identity"));
    }
    Ok(())
}

pub(super) fn require_origin(
    authorization: &Authorization,
    current: &Authorization,
) -> Result<(), OrchestratorError> {
    if authorization == current {
        return Ok(());
    }
    require_completion(authorization, current)
}

pub(super) fn outputs(reference: Option<&ArtifactRef>) -> Result<(), OrchestratorError> {
    let output = reference.map_or_else(|| "resumed=false\n".to_owned(), |reference| format!(
        "resumed=true\noriginal_intent_artifact_id={}\noriginal_intent_artifact_digest=sha256:{}\n", reference.id, reference.sha256));
    let mut file = std::fs::OpenOptions::new()
        .append(true)
        .open(required("GITHUB_OUTPUT")?)
        .map_err(|_| internal("stable_original_intent_output"))?;
    file.write_all(output.as_bytes())
        .map_err(|_| internal("stable_original_intent_output"))
}

fn valid_digest(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
}
