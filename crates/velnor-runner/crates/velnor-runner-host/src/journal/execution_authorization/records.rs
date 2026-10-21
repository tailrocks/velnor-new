use crate::error::HostError;

#[path = "channel_record.rs"]
mod channel_record;
pub(crate) use channel_record::ProofChannelRecord;
pub(crate) use channel_record::publication_matches_channel_directory;

pub(crate) enum ExecutionProofProgress {
    SourceRecorded,
    WaitingForSource,
    WaitingForStarted,
    DecisionPrepared,
    DecisionAlreadyPrepared,
}

pub(crate) struct ExecutionChallengePublicationClaim {
    pub(super) connection_id: String,
    pub(super) reservation_id: i64,
    pub(super) operation_id: i64,
    pub(super) scale_set_id: i64,
    pub(super) witness_request_id: i64,
    pub(super) engine_id: String,
    pub(super) docker_root_digest: String,
    pub(super) runner_id: i64,
    pub(super) runner_name: String,
    pub(super) runner_container_id: String,
    pub(super) dind_container_id: String,
    pub(super) instance_binding_digest: String,
    pub(super) channel_json: String,
    pub(super) challenge: String,
    pub(super) generation: i64,
    pub(super) policy_digest: String,
    pub(super) channel_digest: String,
    pub(super) challenge_json: String,
    pub(super) challenge_digest: String,
}

impl ExecutionChallengePublicationClaim {
    pub(crate) const fn reservation_id(&self) -> i64 {
        self.reservation_id
    }

    pub(crate) const fn operation_id(&self) -> i64 {
        self.operation_id
    }

    pub(crate) fn connection_id(&self) -> &str {
        &self.connection_id
    }

    pub(crate) const fn scale_set_id(&self) -> i64 {
        self.scale_set_id
    }

    pub(crate) const fn witness_request_id(&self) -> i64 {
        self.witness_request_id
    }

    pub(crate) fn engine_id(&self) -> &str {
        &self.engine_id
    }

    pub(crate) fn docker_root_digest(&self) -> &str {
        &self.docker_root_digest
    }

    pub(crate) const fn runner_id(&self) -> i64 {
        self.runner_id
    }

    pub(crate) fn runner_name(&self) -> &str {
        &self.runner_name
    }

    pub(crate) fn runner_container_id(&self) -> &str {
        &self.runner_container_id
    }

    pub(crate) fn dind_container_id(&self) -> &str {
        &self.dind_container_id
    }

    pub(crate) fn instance_binding_digest(&self) -> &str {
        &self.instance_binding_digest
    }

    pub(crate) fn channel_json(&self) -> &str {
        &self.channel_json
    }

    pub(crate) fn challenge(&self) -> &str {
        &self.challenge
    }

    pub(crate) const fn generation(&self) -> i64 {
        self.generation
    }

    pub(crate) fn policy_digest(&self) -> &str {
        &self.policy_digest
    }

    pub(crate) fn channel_digest(&self) -> &str {
        &self.channel_digest
    }

    pub(crate) fn canonical_bytes(&self) -> &[u8] {
        self.challenge_json.as_bytes()
    }

    pub(crate) fn challenge_digest(&self) -> &str {
        &self.challenge_digest
    }
}

pub(crate) struct ExecutionDecisionDeliveryClaim {
    pub(super) connection_id: String,
    pub(super) reservation_id: i64,
    pub(super) operation_id: i64,
    pub(super) scale_set_id: i64,
    pub(super) witness_request_id: i64,
    pub(super) actual_request_id: i64,
    pub(super) engine_id: String,
    pub(super) docker_root_digest: String,
    pub(super) runner_id: i64,
    pub(super) runner_name: String,
    pub(super) runner_container_id: String,
    pub(super) dind_container_id: String,
    pub(super) instance_binding_digest: String,
    pub(super) challenge: String,
    pub(super) generation: i64,
    pub(super) policy_digest: String,
    pub(super) request_digest: String,
    pub(super) decision_digest: String,
    pub(super) channel_digest: String,
    pub(super) channel_json: String,
    pub(super) decision_json: String,
}

impl ExecutionDecisionDeliveryClaim {
    pub(crate) const fn reservation_id(&self) -> i64 {
        self.reservation_id
    }

    pub(crate) const fn operation_id(&self) -> i64 {
        self.operation_id
    }

    pub(crate) fn connection_id(&self) -> &str {
        &self.connection_id
    }

    pub(crate) const fn scale_set_id(&self) -> i64 {
        self.scale_set_id
    }

    pub(crate) const fn witness_request_id(&self) -> i64 {
        self.witness_request_id
    }

    pub(crate) const fn actual_request_id(&self) -> i64 {
        self.actual_request_id
    }

    pub(crate) fn engine_id(&self) -> &str {
        &self.engine_id
    }

    pub(crate) fn docker_root_digest(&self) -> &str {
        &self.docker_root_digest
    }

    pub(crate) const fn runner_id(&self) -> i64 {
        self.runner_id
    }

    pub(crate) fn runner_name(&self) -> &str {
        &self.runner_name
    }

    pub(crate) fn runner_container_id(&self) -> &str {
        &self.runner_container_id
    }

    pub(crate) fn dind_container_id(&self) -> &str {
        &self.dind_container_id
    }

    pub(crate) fn instance_binding_digest(&self) -> &str {
        &self.instance_binding_digest
    }

    pub(crate) fn challenge(&self) -> &str {
        &self.challenge
    }

    pub(crate) const fn generation(&self) -> i64 {
        self.generation
    }

    pub(crate) fn policy_digest(&self) -> &str {
        &self.policy_digest
    }

    pub(crate) fn request_digest(&self) -> &str {
        &self.request_digest
    }

    pub(crate) fn decision_digest(&self) -> &str {
        &self.decision_digest
    }

    pub(crate) fn channel_digest(&self) -> &str {
        &self.channel_digest
    }

    pub(crate) fn channel_json(&self) -> &str {
        &self.channel_json
    }

    pub(crate) fn canonical_bytes(&self) -> &[u8] {
        self.decision_json.as_bytes()
    }
}

pub(super) fn digest(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(64);
    for byte in Sha256::digest(bytes) {
        output.push(char::from(HEX[usize::from(byte >> 4)]));
        output.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    output
}

pub(super) fn sql_i64(value: u64) -> Result<i64, HostError> {
    i64::try_from(value).map_err(|_| HostError::Journal)
}
