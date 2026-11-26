// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::time::Instant;

use velnor_runner_github::workflow_run::{ComparedHookPullRequestFacts, ComparedHookSourceFacts};

use crate::config::{ApprovedJobAuthorityPolicy, RepositoryConnection};
use crate::error::HostError;
use crate::journal::execution_authorization::NativeStartedSourceContext;
use crate::worker::execution_authorization::VerifiedProofChannelBinding;

#[path = "verifier/claim.rs"]
mod claim;
#[path = "verifier/workflow_verification.rs"]
mod workflow_verification;
pub(crate) use claim::{PullClaim, SourceClaim, verify_source_claim};
use workflow_verification::VerifiedWorkflowClosure;

use super::{
    AuthorizedExecutionSource, ExecutionSourceRequest, PersistedExecutionSource,
    PersistedPullRequestSource,
};

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
    let claim = claim_of(request, native)?;
    let verified = verify_source_claim(connection, policy, &claim, token, deadline)?;
    let source = persist_verified_source(request, channel, &verified.compared, verified.closure)?;
    if Instant::now() >= deadline {
        return Err(HostError::Identity);
    }
    Ok(source)
}

fn claim_of(
    request: &ExecutionSourceRequest,
    native: &NativeStartedSourceContext,
) -> Result<SourceClaim, HostError> {
    let pull_request = request
        .pull_request_number()
        .map(|number| -> Result<PullClaim, HostError> {
            Ok(PullClaim {
                number,
                base_ref: request.base_ref().ok_or(HostError::Identity)?.to_owned(),
                base_sha: request.base_sha().ok_or(HostError::Identity)?.to_owned(),
                head_ref: request.head_ref().ok_or(HostError::Identity)?.to_owned(),
                head_sha: request.head_sha().ok_or(HostError::Identity)?.to_owned(),
            })
        })
        .transpose()?;
    Ok(SourceClaim {
        repository_id: request.repository_id(),
        repository_name: request.repository_name().to_owned(),
        run_id: request.run_id(),
        run_attempt: request.run_attempt(),
        event_name: request.event_name().to_owned(),
        event_ref: request.event_ref().to_owned(),
        tested_sha: request.tested_sha().to_owned(),
        workflow_ref: request.workflow_ref().to_owned(),
        workflow_sha: request.workflow_sha().to_owned(),
        job_workflow_ref: native.job_workflow_ref().to_owned(),
        workflow_source_sha: native.workflow_source_sha().map(str::to_owned),
        pull_request,
    })
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
