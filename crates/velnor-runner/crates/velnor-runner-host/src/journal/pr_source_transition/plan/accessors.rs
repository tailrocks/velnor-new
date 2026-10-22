use super::TransitionPlan;
use velnor_runner_github::workflow_run::GitCommitIdentity;

impl TransitionPlan {
    /// Connection identifier bound to this transition.
    #[must_use]
    pub fn connection_id(&self) -> &str {
        &self.connection_id
    }

    /// Configured scale set identifier.
    #[must_use]
    pub const fn scale_set_id(&self) -> i64 {
        self.scale_set_id
    }

    /// Digest of the immutable connection binding.
    #[must_use]
    pub fn binding_digest(&self) -> &str {
        &self.binding_digest
    }

    /// Base repository identifier.
    #[must_use]
    pub const fn repository_id(&self) -> i64 {
        self.repository_id
    }

    /// Base repository owner.
    #[must_use]
    pub fn repository_owner(&self) -> &str {
        &self.repository_owner
    }

    /// Base repository name.
    #[must_use]
    pub fn repository_name(&self) -> &str {
        &self.repository_name
    }

    /// Pull request number.
    #[must_use]
    pub const fn pull_request_number(&self) -> i64 {
        self.pull_request_number
    }

    /// Head repository identifier.
    #[must_use]
    pub const fn head_repository_id(&self) -> i64 {
        self.head_repository_id
    }

    /// Exact pull request head branch.
    #[must_use]
    pub fn head_ref(&self) -> &str {
        &self.head_ref
    }

    /// Exact pull request head commit.
    #[must_use]
    pub fn head_sha(&self) -> &str {
        &self.head_sha
    }

    /// Tree SHA for the exact pull request head commit.
    #[must_use]
    pub fn head_tree_sha(&self) -> &str {
        &self.head_tree_sha
    }

    /// Base branch before the transition.
    #[must_use]
    pub fn previous_base_ref(&self) -> &str {
        &self.previous_base_ref
    }

    /// Base commit before the transition.
    #[must_use]
    pub fn previous_base_sha(&self) -> &str {
        &self.previous_base_sha
    }

    /// Digest of the complete prior base-change timeline.
    #[must_use]
    pub fn base_history_digest(&self) -> &str {
        &self.base_history_digest
    }

    /// Event count in the complete prior base-change timeline.
    #[must_use]
    pub const fn base_history_event_count(&self) -> u32 {
        self.base_history_event_count
    }

    /// Requested new base branch.
    #[must_use]
    pub fn target_base_ref(&self) -> &str {
        &self.target_base_ref
    }

    /// Digest of the policy that authorized the transition.
    #[must_use]
    pub fn policy_digest(&self) -> &str {
        &self.policy_digest
    }

    /// Configured author and committer identity for the nonce commit.
    #[must_use]
    pub const fn commit_identity(&self) -> &GitCommitIdentity {
        &self.commit_identity
    }
}
