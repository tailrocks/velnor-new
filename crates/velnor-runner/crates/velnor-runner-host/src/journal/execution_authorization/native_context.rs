//! Trusted native workflow facts from one acquired Runner request.

use crate::error::HostError;
use crate::journal::connection::ConnectionJournal;
use crate::journal::connection::storage::in_transaction;
use crate::launch::execution_authorization::ExecutionSourceRequest;
use crate::worker::execution_authorization::VerifiedProofChannelBinding;

use super::decision::load_started;

/// Native workflow identity that passed the durable Started-request checks.
pub(crate) struct NativeStartedSourceContext {
    job_workflow_ref: String,
    workflow_source_sha: Option<String>,
}

impl NativeStartedSourceContext {
    pub(crate) fn job_workflow_ref(&self) -> &str {
        &self.job_workflow_ref
    }

    pub(crate) fn workflow_source_sha(&self) -> Option<&str> {
        self.workflow_source_sha.as_deref()
    }
}

pub(super) async fn load(
    scope: &ConnectionJournal,
    request: &ExecutionSourceRequest,
    channel: &VerifiedProofChannelBinding,
) -> Result<NativeStartedSourceContext, HostError> {
    if !channel.valid()
        || request.intent_id() != channel.operation_id()
        || request.reservation_id() != channel.reservation_id()
        || channel.connection_id() != scope.connection_id().as_str()
    {
        return Err(HostError::Identity);
    }
    let operation_id = request.intent_id();
    let connection_id = scope.connection_id().as_str().to_owned();
    in_transaction(scope.journal_ref(), async |connection| {
        scope.validate_scope(connection).await?;
        let started = load_started(connection, operation_id, &connection_id)
            .await?
            .ok_or(HostError::Identity)?;
        validate_started(&started, request, channel)
    })
    .await
}

pub(super) fn validate_started(
    started: &super::decision::StartedExecution,
    request: &ExecutionSourceRequest,
    channel: &VerifiedProofChannelBinding,
) -> Result<NativeStartedSourceContext, HostError> {
    let repository_id = i64::try_from(request.repository_id()).map_err(|_| HostError::Identity)?;
    let source = started.validated_source(repository_id)?;
    let expected_head_sha = request.head_sha().unwrap_or_else(|| request.tested_sha());
    let (owner, repository) = request
        .repository_name()
        .split_once('/')
        .ok_or(HostError::Identity)?;
    if started.jit_witness_request_id() != channel.witness_request_id()
        || started.jit_started_request_id() != Some(started.request_id())
        || started.jit_state() != "started"
        || started.acquire_state() != "acquired"
        || started.runner_terminated()
        || started.runner_id() != channel.runner_id()
        || started.runner_name() != channel.runner_name()
        || started.jit_runner_id() != Some(channel.runner_id())
        || started.jit_runner_name() != Some(channel.runner_name())
        || source.repository_id != repository_id
        || source.owner_name != owner
        || source.repository_name != repository
        || source.workflow_run_id
            != i64::try_from(request.run_id()).map_err(|_| HostError::Identity)?
        || source.run_attempt != i64::from(request.run_attempt())
        || source.event_name != request.event_name()
        || source.head_sha != expected_head_sha
        || source.workflow.owner_name != owner
        || source.workflow.repository_name != repository
        || source.job_workflow_ref != source.workflow.full_ref
        || source.job_workflow_ref
            != format!(
                "{}/{}/{}@{}",
                source.workflow.owner_name,
                source.workflow.repository_name,
                source.workflow.workflow_path,
                source.workflow.reference
            )
    {
        return Err(HostError::Identity);
    }
    let root_execution = source.job_workflow_ref == request.workflow_ref();
    if (root_execution
        && source
            .workflow_source_sha
            .as_deref()
            .is_some_and(|sha| sha != request.workflow_sha()))
        || (!root_execution && source.workflow_source_sha.is_none())
    {
        return Err(HostError::Identity);
    }
    Ok(NativeStartedSourceContext {
        job_workflow_ref: source.job_workflow_ref,
        workflow_source_sha: source.workflow_source_sha,
    })
}
