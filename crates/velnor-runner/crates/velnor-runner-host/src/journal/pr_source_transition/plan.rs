use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use velnor_runner_github::workflow_run::GitCommitIdentity;

use super::types::TransitionError;
use super::validation::{
    DIGEST_HEX_LEN, valid_hex, valid_owner_or_repository, valid_ref, valid_sha, valid_token,
};

/// Immutable connection, repository, pull request, policy and configured Git identity.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct TransitionPlan {
    pub(crate) connection_id: String,
    pub(crate) scale_set_id: i64,
    pub(crate) binding_digest: String,
    pub(crate) repository_id: i64,
    pub(crate) repository_owner: String,
    pub(crate) repository_name: String,
    pub(crate) pull_request_number: i64,
    pub(crate) head_repository_id: i64,
    pub(crate) head_ref: String,
    pub(crate) head_sha: String,
    pub(crate) previous_base_ref: String,
    pub(crate) previous_base_sha: String,
    pub(crate) base_history_digest: String,
    pub(crate) base_history_event_count: u32,
    pub(crate) target_base_ref: String,
    pub(crate) policy_digest: String,
    pub(crate) commit_identity: GitCommitIdentity,
}

/// Values captured by the trusted source-policy path before any remote transition effect.
pub(crate) struct TransitionPlanInput {
    pub(crate) connection_id: String,
    pub(crate) scale_set_id: i64,
    pub(crate) binding_digest: String,
    pub(crate) repository_id: i64,
    pub(crate) repository_owner: String,
    pub(crate) repository_name: String,
    pub(crate) pull_request_number: i64,
    pub(crate) head_repository_id: i64,
    pub(crate) head_ref: String,
    pub(crate) head_sha: String,
    pub(crate) previous_base_ref: String,
    pub(crate) previous_base_sha: String,
    pub(crate) base_history_digest: String,
    pub(crate) base_history_event_count: u32,
    pub(crate) target_base_ref: String,
    pub(crate) policy_digest: String,
    pub(crate) commit_identity: GitCommitIdentity,
}

impl TransitionPlan {
    /// Validate and freeze an exact transition identity.
    pub(crate) fn try_new(input: TransitionPlanInput) -> Result<Self, TransitionError> {
        let plan = Self {
            connection_id: input.connection_id,
            scale_set_id: input.scale_set_id,
            binding_digest: input.binding_digest,
            repository_id: input.repository_id,
            repository_owner: input.repository_owner,
            repository_name: input.repository_name,
            pull_request_number: input.pull_request_number,
            head_repository_id: input.head_repository_id,
            head_ref: input.head_ref,
            head_sha: input.head_sha,
            previous_base_ref: input.previous_base_ref,
            previous_base_sha: input.previous_base_sha,
            base_history_digest: input.base_history_digest,
            base_history_event_count: input.base_history_event_count,
            target_base_ref: input.target_base_ref,
            policy_digest: input.policy_digest,
            commit_identity: input.commit_identity,
        };
        plan.validate()?;
        Ok(plan)
    }

    /// Stable digest for the exact connection, policy and source transition.
    pub(crate) fn digest(&self) -> Result<String, TransitionError> {
        self.validate()?;
        let mut hash = Sha256::new();
        hash.update(b"velnor-pr-source-transition-plan-v1\0");
        for value in [
            self.scale_set_id,
            self.repository_id,
            self.pull_request_number,
            self.head_repository_id,
            i64::from(self.base_history_event_count),
        ] {
            hash.update(value.to_le_bytes());
        }
        for value in [
            self.connection_id.as_str(),
            self.binding_digest.as_str(),
            self.repository_owner.as_str(),
            self.repository_name.as_str(),
            self.head_ref.as_str(),
            self.head_sha.as_str(),
            self.previous_base_ref.as_str(),
            self.previous_base_sha.as_str(),
            self.base_history_digest.as_str(),
            self.target_base_ref.as_str(),
            self.policy_digest.as_str(),
            self.commit_identity.name(),
            self.commit_identity.email(),
        ] {
            hash_text(&mut hash, value)?;
        }
        Ok(hex(&hash.finalize()))
    }

    pub(crate) fn validate(&self) -> Result<(), TransitionError> {
        if !valid_token(&self.connection_id)
            || self.scale_set_id <= 0
            || !valid_hex(&self.binding_digest, DIGEST_HEX_LEN)
            || self.repository_id <= 0
            || self.pull_request_number <= 0
            || self.head_repository_id != self.repository_id
            || !valid_owner_or_repository(&self.repository_owner)
            || !valid_owner_or_repository(&self.repository_name)
            || !valid_ref(&self.head_ref)
            || !valid_sha(&self.head_sha)
            || !valid_ref(&self.previous_base_ref)
            || !valid_sha(&self.previous_base_sha)
            || !valid_hex(&self.base_history_digest, DIGEST_HEX_LEN)
            || !valid_ref(&self.target_base_ref)
            || self.previous_base_ref == self.target_base_ref
            || !valid_hex(&self.policy_digest, DIGEST_HEX_LEN)
        {
            return Err(TransitionError::IncorrectPlan);
        }
        Ok(())
    }
}

fn hash_text(hash: &mut Sha256, value: &str) -> Result<(), TransitionError> {
    let length = u64::try_from(value.len()).map_err(|_| TransitionError::IncorrectPlan)?;
    hash.update(length.to_le_bytes());
    hash.update(value.as_bytes());
    Ok(())
}

fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut value = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        value.push(char::from(DIGITS[usize::from(byte >> 4)]));
        value.push(char::from(DIGITS[usize::from(byte & 0x0f)]));
    }
    value
}
