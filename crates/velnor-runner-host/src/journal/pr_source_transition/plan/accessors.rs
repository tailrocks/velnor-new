// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::TransitionPlan;
use velnor_runner_github::workflow_run::GitCommitIdentity;

impl TransitionPlan {
    #[must_use]
    pub fn connection_id(&self) -> &str {
        &self.connection_id
    }

    #[must_use]
    pub const fn scale_set_id(&self) -> i64 {
        self.scale_set_id
    }

    #[must_use]
    pub fn binding_digest(&self) -> &str {
        &self.binding_digest
    }

    #[must_use]
    pub const fn repository_id(&self) -> i64 {
        self.repository_id
    }

    #[must_use]
    pub fn repository_owner(&self) -> &str {
        &self.repository_owner
    }

    #[must_use]
    pub fn repository_name(&self) -> &str {
        &self.repository_name
    }

    #[must_use]
    pub const fn pull_request_number(&self) -> i64 {
        self.pull_request_number
    }

    #[must_use]
    pub const fn head_repository_id(&self) -> i64 {
        self.head_repository_id
    }

    #[must_use]
    pub fn head_ref(&self) -> &str {
        &self.head_ref
    }

    #[must_use]
    pub fn head_sha(&self) -> &str {
        &self.head_sha
    }

    #[must_use]
    pub fn head_tree_sha(&self) -> &str {
        &self.head_tree_sha
    }

    #[must_use]
    pub fn previous_base_ref(&self) -> &str {
        &self.previous_base_ref
    }

    #[must_use]
    pub fn previous_base_sha(&self) -> &str {
        &self.previous_base_sha
    }

    #[must_use]
    pub fn base_history_digest(&self) -> &str {
        &self.base_history_digest
    }

    #[must_use]
    pub const fn base_history_event_count(&self) -> u32 {
        self.base_history_event_count
    }

    #[must_use]
    pub fn target_base_ref(&self) -> &str {
        &self.target_base_ref
    }

    #[must_use]
    pub fn policy_digest(&self) -> &str {
        &self.policy_digest
    }

    #[must_use]
    pub const fn commit_identity(&self) -> &GitCommitIdentity {
        &self.commit_identity
    }
}
