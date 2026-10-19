//! Build live source proof from the measured hook request and GitHub records.

use std::time::Instant;

use serde_json::json;
use velnor_runner_github::workflow_run::{
    ComparedHookPullRequestFacts, ComparedHookSourceFacts, RepositoryTarget,
    UntrustedHookSourceInput, WorkflowRun, compare_hook_source, get_workflow_run_attempt,
};

use crate::config::{ApprovedJobAuthorityPolicy, RepositoryConnection};
use crate::error::HostError;
use crate::https::HttpsTransport;
use crate::journal::execution_authorization::NativeStartedSourceContext;
use crate::worker::execution_authorization::VerifiedProofChannelBinding;

#[path = "verifier/workflow_verification.rs"]
mod workflow_verification;
use workflow_verification::{
    VerifiedWorkflowClosure, WorkflowVerification, root_source_commit_matches,
    verify_workflow_closure,
};
#[cfg(test)]
use workflow_verification::{insert_source_ref, source_refs_match_approved_closure};

use super::{
    AuthorizedExecutionSource, ExecutionSourceRequest, PersistedExecutionSource,
    PersistedPullRequestSource,
};

#[cfg(test)]
#[path = "verifier_tests.rs"]
mod tests;

const GITHUB_API: &str = "https://api.github.com";

/// Authenticate one measured hook request against the configured repository and inventory.
pub(crate) fn verify_live_source(
    connection: &RepositoryConnection,
    policy: &ApprovedJobAuthorityPolicy,
    request: &ExecutionSourceRequest,
    channel: &VerifiedProofChannelBinding,
    native: &NativeStartedSourceContext,
    token: &str,
    deadline: Instant,
) -> Result<AuthorizedExecutionSource, HostError> {
    if Instant::now() >= deadline {
        return Err(HostError::Identity);
    }
    policy.validate_for(connection)?;
    if !channel.valid() {
        return Err(HostError::Identity);
    }
    if request.reservation_id() != channel.reservation_id()
        || request.intent_id() != channel.operation_id()
        || request.repository_id() != u64::try_from(connection.repository_id).unwrap_or_default()
        || request.repository_name() != connection.repository
    {
        return Err(HostError::Identity);
    }
    let (owner, repository) = split_repository(&connection.repository)?;
    let target = RepositoryTarget {
        id: connection.repository_id,
        owner,
        name: repository,
    };
    let mut transport = HttpsTransport::with_deadline(GITHUB_API, deadline)?;
    let run = get_workflow_run_attempt(
        &mut transport,
        owner,
        repository,
        i64::try_from(request.run_id()).map_err(|_| HostError::Identity)?,
        i64::from(request.run_attempt()),
        token,
    )
    .map_err(|_| HostError::Identity)?;
    let hook_input = hook_input(request, native, &run)?;
    let compared = compare_hook_source(&mut transport, &hook_input, target, token)
        .map_err(|_| HostError::Identity)?;
    verify_compared_request(request, &compared)?;

    let workflow_closure = verify_workflow_closure(
        WorkflowVerification {
            connection,
            policy,
            request,
            native,
            owner,
            repository,
            target,
            run: &run,
            compared: &compared,
        },
        &mut transport,
        token,
    )?;
    let source = persist_verified_source(request, channel, &compared, workflow_closure)?;
    if Instant::now() >= deadline {
        return Err(HostError::Identity);
    }
    Ok(source)
}

fn persist_verified_source(
    request: &ExecutionSourceRequest,
    channel: &VerifiedProofChannelBinding,
    compared: &ComparedHookSourceFacts,
    closure: VerifiedWorkflowClosure,
) -> Result<AuthorizedExecutionSource, HostError> {
    let root = closure
        .identities
        .iter()
        .find(|identity| identity.path == closure.root_path)
        .ok_or(HostError::Identity)?;
    let leaf = closure
        .identities
        .iter()
        .find(|identity| identity.path == closure.leaf_path)
        .ok_or(HostError::Identity)?;
    let pull_request_source = compared
        .pull_request()
        .map(|pull| make_pull_request_source(request, pull))
        .transpose()?;
    let record = PersistedExecutionSource {
        request_digest: request.sha256()?,
        tested_commit_sha: request.tested_sha().to_owned(),
        root_workflow_path: root.path.clone(),
        root_commit_sha: root.commit_sha.clone(),
        root_tree_sha: root.tree_sha.clone(),
        root_blob_sha: root.blob_sha.clone(),
        root_content_sha256: root.content_sha256.clone(),
        leaf_workflow_path: leaf.path.clone(),
        leaf_commit_sha: leaf.commit_sha.clone(),
        leaf_tree_sha: leaf.tree_sha.clone(),
        leaf_blob_sha: leaf.blob_sha.clone(),
        leaf_content_sha256: leaf.content_sha256.clone(),
        workflow_closure: closure.identities,
        pull_request_source,
        rest_job_id: None,
        request: request.clone(),
    };
    record.validate()?;
    let source = AuthorizedExecutionSource {
        record,
        channel_digest: channel.channel_digest().to_owned(),
    };
    source.validate()?;
    if !source.matches_channel(channel) {
        return Err(HostError::Identity);
    }
    Ok(source)
}

fn split_repository(repository: &str) -> Result<(&str, &str), HostError> {
    let Some((owner, name)) = repository.split_once('/') else {
        return Err(HostError::Identity);
    };
    if owner.is_empty() || name.is_empty() || name.contains('/') {
        return Err(HostError::Identity);
    }
    Ok((owner, name))
}

fn hook_input(
    request: &ExecutionSourceRequest,
    native: &NativeStartedSourceContext,
    run: &WorkflowRun,
) -> Result<UntrustedHookSourceInput, HostError> {
    let pull_request = if let Some(number) = request.pull_request_number() {
        let number = i64::try_from(number).map_err(|_| HostError::Identity)?;
        let mut matches = run
            .pull_requests
            .iter()
            .filter(|pull_request| pull_request.number == number);
        let pull = matches.next().ok_or(HostError::Identity)?;
        if matches.next().is_some() {
            return Err(HostError::Identity);
        }
        let base = pull.base.repository.as_ref().ok_or(HostError::Identity)?;
        let head = pull.head.repository.as_ref().ok_or(HostError::Identity)?;
        let head_repository = run
            .head_repository
            .as_ref()
            .filter(|repository| repository.id == head.id)
            .ok_or(HostError::Identity)?;
        Some(json!({
            "number": number,
            "base": {
                "repository": {
                    "id": base.id,
                    "name": base.name,
                    "full_name": format!("{}/{}", run.repository.owner.login, run.repository.name)
                },
                "ref": request.base_ref().ok_or(HostError::Identity)?,
                "sha": request.base_sha().ok_or(HostError::Identity)?
            },
            "head": {
                "repository": {
                    "id": head.id,
                    "name": head.name,
                    "full_name": head_repository.full_name
                },
                "ref": request.head_ref().ok_or(HostError::Identity)?,
                "sha": request.head_sha().ok_or(HostError::Identity)?
            }
        }))
    } else {
        None
    };
    let value = json!({
        "repository_id": request.repository_id(),
        "repository": request.repository_name(),
        "run_id": request.run_id(),
        "run_attempt": request.run_attempt(),
        "event_name": request.event_name(),
        "event_ref": request.event_ref(),
        "workflow_ref": request.workflow_ref(),
        "workflow_sha": request.workflow_sha(),
        "tested_sha": request.tested_sha(),
        "job_workflow_ref": native.job_workflow_ref(),
        "workflow_job_id": request.job_key(),
        "pull_request": pull_request
    });
    serde_json::from_value(value).map_err(|_| HostError::Identity)
}

fn verify_compared_request(
    request: &ExecutionSourceRequest,
    compared: &ComparedHookSourceFacts,
) -> Result<(), HostError> {
    let (run_id, attempt) = compared.run_identity();
    let (event, event_ref, tested_sha) = compared.event_source();
    let (workflow_ref, workflow_sha) = compared.workflow_source();
    if u64::try_from(run_id).ok() != Some(request.run_id())
        || u32::try_from(attempt).ok() != Some(request.run_attempt())
        || event != request.event_name()
        || event_ref != request.event_ref()
        || tested_sha != request.tested_sha()
        || workflow_ref != request.workflow_ref()
        || workflow_sha != request.workflow_sha()
        || !root_source_commit_matches(request)
        || compared.workflow_job_id() != request.job_key()
    {
        return Err(HostError::Identity);
    }
    Ok(())
}

fn make_pull_request_source(
    request: &ExecutionSourceRequest,
    pull: &ComparedHookPullRequestFacts,
) -> Result<PersistedPullRequestSource, HostError> {
    let (
        number,
        base_repository_id,
        base_repository_name,
        base_ref,
        head_repository_id,
        head_repository_name,
        head_ref,
        _,
    ) = pull.identity();
    let (base_sha, head_sha) = pull.commit_shas();
    if Some(u64::try_from(number).map_err(|_| HostError::Identity)?)
        != request.pull_request_number()
        || Some(base_ref) != request.base_ref()
        || Some(base_sha) != request.base_sha()
        || Some(head_ref) != request.head_ref()
        || Some(head_sha) != request.head_sha()
    {
        return Err(HostError::Identity);
    }
    Ok(PersistedPullRequestSource {
        number: u64::try_from(number).map_err(|_| HostError::Identity)?,
        base_repository_id,
        base_repository_name: base_repository_name.to_owned(),
        base_ref: base_ref.to_owned(),
        base_sha: base_sha.to_owned(),
        head_repository_id,
        head_repository_name: head_repository_name.to_owned(),
        head_ref: head_ref.to_owned(),
        head_sha: head_sha.to_owned(),
        merge_ref: request.event_ref().to_owned(),
        merge_commit_sha: request.tested_sha().to_owned(),
    })
}
