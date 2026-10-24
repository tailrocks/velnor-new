//! Durable policy, challenge, source-proof, and decision records.

mod challenge;
mod decision;
mod native_context;
mod policy;
mod proof;
mod records;
mod recovery;
mod timeout;

pub(crate) use records::{
    ExecutionChallengePublicationClaim, ExecutionDecisionDeliveryClaim, ExecutionProofProgress,
    ProofChannelRecord,
};

use crate::config::{ApprovedJobAuthorityPolicy, HostConfig};
use crate::error::HostError;
use crate::journal::connection::ConnectionJournal;
use crate::launch::execution_authorization::AuthorizedExecutionSource;
use crate::launch::execution_authorization::ExecutionSourceRequest;
use crate::worker::execution_authorization::{
    ExecutionChallengePublicationReceipt, ExecutionDecisionDeliveryReceipt,
    VerifiedProofChannelBinding,
};

pub(crate) use native_context::NativeStartedSourceContext;
pub(crate) use timeout::StartedRequestTimeoutResult;

impl ConnectionJournal {
    pub(crate) async fn deny_started_request_timeout(
        &self,
        channel: &VerifiedProofChannelBinding,
        actual_request_id: i64,
    ) -> Result<StartedRequestTimeoutResult, HostError> {
        timeout::deny_started_request_timeout(self, channel, actual_request_id).await
    }

    pub(crate) async fn native_started_source_context(
        &self,
        request: &ExecutionSourceRequest,
        channel: &VerifiedProofChannelBinding,
    ) -> Result<NativeStartedSourceContext, HostError> {
        native_context::load(self, request, channel).await
    }

    pub(crate) async fn install_job_authority_policy(
        &self,
        config: &HostConfig,
    ) -> Result<(), HostError> {
        let policy = config
            .validated_job_authority_policy(self.connection_id())?
            .ok_or(HostError::Identity)?;
        policy::install(self, config, policy).await
    }

    pub(crate) async fn claim_execution_challenge(
        &self,
        reservation_id: i64,
        channel: &VerifiedProofChannelBinding,
    ) -> Result<Option<ExecutionChallengePublicationClaim>, HostError> {
        challenge::claim(self, reservation_id, channel).await
    }

    pub(crate) async fn record_execution_challenge_published(
        &self,
        claim: &ExecutionChallengePublicationClaim,
        receipt: &ExecutionChallengePublicationReceipt,
    ) -> Result<(), HostError> {
        challenge::record_published(self, claim, receipt).await
    }

    pub(crate) async fn mark_execution_challenge_uncertain(
        &self,
        claim: &ExecutionChallengePublicationClaim,
    ) -> Result<(), HostError> {
        challenge::mark_uncertain(self, claim).await
    }

    pub(crate) async fn record_authorized_execution_source(
        &self,
        source: &AuthorizedExecutionSource,
    ) -> Result<ExecutionProofProgress, HostError> {
        proof::record_source(self, source).await
    }

    pub(crate) async fn prepare_execution_decision(
        &self,
        source: &AuthorizedExecutionSource,
    ) -> Result<ExecutionProofProgress, HostError> {
        decision::prepare(self, source).await
    }

    pub(crate) async fn claim_execution_decision_delivery(
        &self,
        operation_id: i64,
    ) -> Result<Option<ExecutionDecisionDeliveryClaim>, HostError> {
        decision::claim_delivery(self, operation_id).await
    }

    pub(crate) async fn record_execution_decision_delivered(
        &self,
        claim: &ExecutionDecisionDeliveryClaim,
        receipt: &ExecutionDecisionDeliveryReceipt,
    ) -> Result<(), HostError> {
        decision::record_delivered(self, claim, receipt).await
    }

    pub(crate) async fn mark_execution_decision_uncertain(
        &self,
        claim: &ExecutionDecisionDeliveryClaim,
    ) -> Result<(), HostError> {
        decision::mark_uncertain(self, claim).await
    }
}

/// Reconcile all interrupted proof effects inside the existing host startup transaction.
pub(crate) async fn recover_at_startup(connection: &turso::Connection) -> Result<(), HostError> {
    recovery::recover(connection).await
}

fn policy_digest(policy: &ApprovedJobAuthorityPolicy) -> Result<String, HostError> {
    let bytes = policy.canonical_json().map_err(|_| HostError::Journal)?;
    Ok(records::digest(&bytes))
}
