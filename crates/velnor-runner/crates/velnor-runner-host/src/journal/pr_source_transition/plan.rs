use serde::de;
use serde::{Deserialize, Deserializer, Serialize};
use sha2::{Digest, Sha256};
use velnor_runner_github::workflow_run::{GitCommitIdentity, PullRequestBaseHistory};

use super::receipt::base_history_fingerprint;
use super::types::TransitionError;
use super::validation::{
    DIGEST_HEX_LEN, valid_hex, valid_owner_or_repository, valid_ref, valid_sha, valid_token,
};

#[path = "plan/accessors.rs"]
mod accessors;

const MAX_BASE_HISTORY_EVENTS: u32 = 6400;

/// Immutable connection, repository, pull request, policy and configured Git identity.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TransitionPlan {
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
    pub(crate) head_tree_sha: String,
    pub(crate) previous_base_ref: String,
    pub(crate) previous_base_sha: String,
    pub(crate) base_history_digest: String,
    pub(crate) base_history_event_count: u32,
    pub(crate) target_base_ref: String,
    pub(crate) policy_digest: String,
    pub(crate) commit_identity: GitCommitIdentity,
}

impl<'de> Deserialize<'de> for TransitionPlan {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct RawIdentity {
            name: String,
            email: String,
        }

        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct RawPlan {
            connection_id: String,
            scale_set_id: i64,
            binding_digest: String,
            repository_id: i64,
            repository_owner: String,
            repository_name: String,
            pull_request_number: i64,
            head_repository_id: i64,
            head_ref: String,
            head_sha: String,
            head_tree_sha: String,
            previous_base_ref: String,
            previous_base_sha: String,
            base_history_digest: String,
            base_history_event_count: u32,
            target_base_ref: String,
            policy_digest: String,
            commit_identity: RawIdentity,
        }

        let raw = RawPlan::deserialize(deserializer)?;
        let identity =
            GitCommitIdentity::try_new(raw.commit_identity.name, raw.commit_identity.email)
                .map_err(de::Error::custom)?;
        let plan = Self {
            connection_id: raw.connection_id,
            scale_set_id: raw.scale_set_id,
            binding_digest: raw.binding_digest,
            repository_id: raw.repository_id,
            repository_owner: raw.repository_owner,
            repository_name: raw.repository_name,
            pull_request_number: raw.pull_request_number,
            head_repository_id: raw.head_repository_id,
            head_ref: raw.head_ref,
            head_sha: raw.head_sha,
            head_tree_sha: raw.head_tree_sha,
            previous_base_ref: raw.previous_base_ref,
            previous_base_sha: raw.previous_base_sha,
            base_history_digest: raw.base_history_digest,
            base_history_event_count: raw.base_history_event_count,
            target_base_ref: raw.target_base_ref,
            policy_digest: raw.policy_digest,
            commit_identity: identity,
        };
        plan.validate().map_err(de::Error::custom)?;
        Ok(plan)
    }
}

/// Values captured by the trusted source-policy path before any remote transition effect.
#[derive(Debug)]
pub struct TransitionPlanInput {
    /// Durable connection identifier.
    pub connection_id: String,
    /// Configured scale set identifier.
    pub scale_set_id: i64,
    /// Digest of the immutable connection binding.
    pub binding_digest: String,
    /// Base repository identifier.
    pub repository_id: i64,
    /// Base repository owner.
    pub repository_owner: String,
    /// Base repository name.
    pub repository_name: String,
    /// Pull request number.
    pub pull_request_number: i64,
    /// Head repository identifier. Must equal the base repository identifier.
    pub head_repository_id: i64,
    /// Exact pull request head branch.
    pub head_ref: String,
    /// Exact pull request head commit.
    pub head_sha: String,
    /// Tree SHA for the exact pull request head commit.
    pub head_tree_sha: String,
    /// Base branch before the transition.
    pub previous_base_ref: String,
    /// Base commit before the transition.
    pub previous_base_sha: String,
    /// Complete base-change timeline observed before any remote effect.
    pub base_history: PullRequestBaseHistory,
    /// Requested new base branch.
    pub target_base_ref: String,
    /// Digest of the policy that authorized the transition.
    pub policy_digest: String,
    /// Configured author and committer identity for the nonce commit.
    pub commit_identity: GitCommitIdentity,
}

impl TransitionPlan {
    /// Validate and freeze an exact transition identity.
    ///
    /// # Errors
    ///
    /// Returns an error if the supplied repository, history, policy or commit identity does not
    /// describe a valid same-repository source transition.
    pub fn try_new(input: TransitionPlanInput) -> Result<Self, TransitionError> {
        let history = &input.base_history;
        if history.number != input.pull_request_number
            || history.repository_id != input.repository_id
            || history.base_repository_id != input.repository_id
            || history.head_repository_id != input.head_repository_id
            || history.head_ref != input.head_ref
            || history.head_sha != input.head_sha
            || history.base_ref != input.previous_base_ref
            || history.base_sha != input.previous_base_sha
        {
            return Err(TransitionError::IncorrectPlan);
        }
        let (base_history_event_count, base_history_digest) = base_history_fingerprint(history)?;
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
            head_tree_sha: input.head_tree_sha,
            previous_base_ref: input.previous_base_ref,
            previous_base_sha: input.previous_base_sha,
            base_history_digest,
            base_history_event_count,
            target_base_ref: input.target_base_ref,
            policy_digest: input.policy_digest,
            commit_identity: input.commit_identity,
        };
        plan.validate()?;
        Ok(plan)
    }

    /// Stable digest for the exact connection, policy and source transition.
    ///
    /// # Errors
    ///
    /// Returns an error if any frozen plan field is invalid.
    pub fn digest(&self) -> Result<String, TransitionError> {
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
            self.head_tree_sha.as_str(),
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

    /// Stable identity for one connection, repository, pull request and exact head commit.
    ///
    /// Binding, Scale Set and policy facts are deliberately excluded so changing local
    /// configuration cannot start a second effect sequence for the same remote source.
    ///
    /// # Errors
    ///
    /// Returns an error if any frozen plan field is invalid.
    pub fn transition_id(&self) -> Result<String, TransitionError> {
        self.validate()?;
        let mut hash = Sha256::new();
        hash.update(b"velnor-pr-source-transition-id-v1\0");
        for value in [
            self.repository_id,
            self.pull_request_number,
            self.head_repository_id,
        ] {
            hash.update(value.to_le_bytes());
        }
        for value in [
            self.connection_id.as_str(),
            self.head_ref.as_str(),
            self.head_sha.as_str(),
        ] {
            hash_text(&mut hash, value)?;
        }
        Ok(hex(&hash.finalize()))
    }

    /// Validate every frozen identity and digest field.
    ///
    /// # Errors
    ///
    /// Returns an error if any frozen plan field is invalid.
    pub fn validate(&self) -> Result<(), TransitionError> {
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
            || !valid_sha(&self.head_tree_sha)
            || !valid_ref(&self.previous_base_ref)
            || !valid_sha(&self.previous_base_sha)
            || !valid_hex(&self.base_history_digest, DIGEST_HEX_LEN)
            || self.base_history_event_count > MAX_BASE_HISTORY_EVENTS
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
