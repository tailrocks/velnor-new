//! Opaque receipts for the Runner source-proof channel.

mod channel;
mod publication;

pub(crate) use channel::{
    ExecutionRequestRead, ProofChannelMeasurement, VerifiedExecutionRequest,
    VerifiedProofChannelBinding, measure_proof_channel, read_execution_request,
};
#[cfg(test)]
pub(crate) use channel::{
    TestProofChannelBindingFields, TestProofChannelIdentity, TestProofChannelMount,
    test_channel_for_ready_job_facts, test_proof_channel_mount,
    test_verified_proof_channel_binding,
};
pub(crate) use publication::{
    ExecutionChallengePublicationReceipt, ExecutionDecisionDeliveryReceipt, VerifiedMetadataVolume,
    challenge_publication_receipt, decision_delivery_receipt, inspect_metadata_volume,
};
#[cfg(test)]
pub(crate) use publication::{TestControlFile, test_execution_challenge_publication_receipt};
