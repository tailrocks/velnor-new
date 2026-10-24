//! Canonical challenge and decision documents.

use serde::{Deserialize, Serialize};

use crate::error::HostError;

use super::{digest, optional_text, valid_deny_code, valid_digest, valid_object_id, valid_text};

/// One challenge published to the fixed root-owned control directory.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ExecutionProofChallenge {
    schema_version: u32,
    challenge: String,
    reservation_id: i64,
    intent_id: i64,
    witness_request_id: i64,
    runner_id: i64,
    runner_name: String,
    runner_scale_set_id: i64,
    runner_container_id: String,
    dind_container_id: String,
    engine_id: String,
    docker_root_digest: String,
    instance_binding_digest: String,
    policy_generation: u64,
    policy_digest: String,
    channel_digest: String,
}

pub(crate) struct ExecutionProofChallengeFields {
    pub(crate) challenge: String,
    pub(crate) reservation_id: i64,
    pub(crate) intent_id: i64,
    pub(crate) witness_request_id: i64,
    pub(crate) runner_id: i64,
    pub(crate) runner_name: String,
    pub(crate) runner_scale_set_id: i64,
    pub(crate) runner_container_id: String,
    pub(crate) dind_container_id: String,
    pub(crate) engine_id: String,
    pub(crate) docker_root_digest: String,
    pub(crate) instance_binding_digest: String,
    pub(crate) policy_generation: u64,
    pub(crate) policy_digest: String,
    pub(crate) channel_digest: String,
}

impl ExecutionProofChallenge {
    pub(crate) fn from_journal(fields: ExecutionProofChallengeFields) -> Result<Self, HostError> {
        let value = Self {
            schema_version: 1,
            challenge: fields.challenge,
            reservation_id: fields.reservation_id,
            intent_id: fields.intent_id,
            witness_request_id: fields.witness_request_id,
            runner_id: fields.runner_id,
            runner_name: fields.runner_name,
            runner_scale_set_id: fields.runner_scale_set_id,
            runner_container_id: fields.runner_container_id,
            dind_container_id: fields.dind_container_id,
            engine_id: fields.engine_id,
            docker_root_digest: fields.docker_root_digest,
            instance_binding_digest: fields.instance_binding_digest,
            policy_generation: fields.policy_generation,
            policy_digest: fields.policy_digest,
            channel_digest: fields.channel_digest,
        };
        if value.valid() {
            Ok(value)
        } else {
            Err(HostError::Journal)
        }
    }

    pub(crate) fn canonical_json(&self) -> Result<Vec<u8>, HostError> {
        serde_json::to_vec(self).map_err(|_| HostError::Journal)
    }

    pub(crate) fn valid(&self) -> bool {
        self.schema_version == 1
            && valid_digest(&self.challenge)
            && self.reservation_id > 0
            && self.intent_id > 0
            && self.witness_request_id > 0
            && self.runner_id > 0
            && valid_text(&self.runner_name, 128)
            && self.runner_scale_set_id > 0
            && valid_object_id(&self.runner_container_id)
            && valid_object_id(&self.dind_container_id)
            && self.runner_container_id != self.dind_container_id
            && valid_text(&self.engine_id, 128)
            && valid_digest(&self.docker_root_digest)
            && valid_digest(&self.instance_binding_digest)
            && self.policy_generation > 0
            && valid_digest(&self.policy_digest)
            && valid_digest(&self.channel_digest)
    }

    pub(crate) fn challenge(&self) -> &str {
        &self.challenge
    }

    pub(crate) const fn reservation_id(&self) -> i64 {
        self.reservation_id
    }

    pub(crate) const fn intent_id(&self) -> i64 {
        self.intent_id
    }

    pub(crate) const fn witness_request_id(&self) -> i64 {
        self.witness_request_id
    }

    pub(crate) const fn runner_id(&self) -> i64 {
        self.runner_id
    }

    pub(crate) fn runner_name(&self) -> &str {
        &self.runner_name
    }

    pub(crate) const fn runner_scale_set_id(&self) -> i64 {
        self.runner_scale_set_id
    }

    pub(crate) fn runner_container_id(&self) -> &str {
        &self.runner_container_id
    }

    pub(crate) fn dind_container_id(&self) -> &str {
        &self.dind_container_id
    }

    pub(crate) fn engine_id(&self) -> &str {
        &self.engine_id
    }

    pub(crate) fn docker_root_digest(&self) -> &str {
        &self.docker_root_digest
    }

    pub(crate) fn instance_binding_digest(&self) -> &str {
        &self.instance_binding_digest
    }

    pub(crate) const fn policy_generation(&self) -> u64 {
        self.policy_generation
    }

    pub(crate) fn policy_digest(&self) -> &str {
        &self.policy_digest
    }

    pub(crate) fn channel_digest(&self) -> &str {
        &self.channel_digest
    }
}

/// Deserialized decision data. This record cannot establish approval by itself.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct PersistedExecutionDecision {
    schema_version: u32,
    allow: bool,
    challenge: String,
    request_sha256: String,
    reservation_id: i64,
    intent_id: i64,
    actual_request_id: i64,
    run_id: u64,
    run_attempt: u32,
    job_key: String,
    runner_id: i64,
    policy_generation: u64,
    policy_digest: String,
    native_job_id: Option<String>,
    rest_job_id: Option<String>,
    deny_code: Option<String>,
}

impl PersistedExecutionDecision {
    pub(crate) fn canonical_json(&self) -> Result<Vec<u8>, HostError> {
        serde_json::to_vec(self).map_err(|_| HostError::Journal)
    }

    pub(crate) fn sha256(&self) -> Result<String, HostError> {
        Ok(digest(&self.canonical_json()?))
    }

    pub(crate) fn allow(&self) -> bool {
        self.allow
    }

    pub(crate) fn challenge(&self) -> &str {
        &self.challenge
    }

    pub(crate) fn request_digest(&self) -> &str {
        &self.request_sha256
    }

    pub(crate) const fn reservation_id(&self) -> i64 {
        self.reservation_id
    }

    pub(crate) const fn intent_id(&self) -> i64 {
        self.intent_id
    }

    pub(crate) const fn actual_request_id(&self) -> i64 {
        self.actual_request_id
    }

    pub(crate) const fn run_id(&self) -> u64 {
        self.run_id
    }

    pub(crate) const fn run_attempt(&self) -> u32 {
        self.run_attempt
    }

    pub(crate) fn job_key(&self) -> &str {
        &self.job_key
    }

    pub(crate) const fn runner_id(&self) -> i64 {
        self.runner_id
    }

    pub(crate) const fn policy_generation(&self) -> u64 {
        self.policy_generation
    }

    pub(crate) fn policy_digest(&self) -> &str {
        &self.policy_digest
    }

    pub(crate) fn native_job_id(&self) -> Option<&str> {
        self.native_job_id.as_deref()
    }

    pub(crate) fn rest_job_id(&self) -> Option<&str> {
        self.rest_job_id.as_deref()
    }
    pub(crate) fn valid(&self) -> bool {
        self.schema_version == 1
            && valid_digest(&self.challenge)
            && valid_digest(&self.request_sha256)
            && self.reservation_id > 0
            && self.intent_id > 0
            && self.actual_request_id > 0
            && self.run_id > 0
            && self.run_attempt > 0
            && valid_text(&self.job_key, 128)
            && self.runner_id > 0
            && self.policy_generation > 0
            && valid_digest(&self.policy_digest)
            && optional_text(self.native_job_id.as_deref(), 256)
            && optional_text(self.rest_job_id.as_deref(), 128)
            && if self.allow {
                self.deny_code.is_none()
                    && self
                        .native_job_id
                        .as_deref()
                        .is_some_and(|value| valid_text(value, 256))
            } else {
                self.deny_code.as_deref().is_some_and(valid_deny_code)
            }
    }
}
