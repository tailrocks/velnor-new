//! Validation of persisted execution-source facts.

use crate::config::{
    ApprovedJobAuthorityPolicy, JobAuthorityLookup, JobAuthorityRole, PullRequestRule,
    RepositoryConnection, WorkflowEvent,
};
use crate::error::HostError;
use crate::worker::execution_authorization::VerifiedProofChannelBinding;

use super::super::{
    ExecutionProofChallenge, MAX_CLOSURE_FILES, MAX_PROOF_BYTES, optional_text, valid_digest,
    valid_git_sha, valid_relative_path, valid_text,
};
use super::{
    AuthorizedExecutionSource, ExecutionSourceRequest, PersistedExecutionSource,
    PersistedPullRequestSource, VerifiedWorkflowIdentity,
};

impl PersistedPullRequestSource {
    fn valid(&self) -> bool {
        self.number > 0
            && self.base_repository_id > 0
            && valid_repository_name(&self.base_repository_name)
            && valid_text(&self.base_ref, 1024)
            && valid_git_sha(&self.base_sha)
            && self.head_repository_id > 0
            && valid_repository_name(&self.head_repository_name)
            && valid_text(&self.head_ref, 1024)
            && valid_git_sha(&self.head_sha)
            && valid_text(&self.merge_ref, 128)
            && valid_git_sha(&self.merge_commit_sha)
            && self.merge_ref == format!("refs/pull/{}/merge", self.number)
    }

    fn policy_rule(&self) -> PullRequestRule {
        PullRequestRule {
            number: Some(self.number),
            base_repository_id: self.base_repository_id,
            base_branch: self.base_ref.clone(),
            head_repository_id: self.head_repository_id,
            head_branch: self.head_ref.clone(),
        }
    }
}

impl PersistedExecutionSource {
    pub(crate) fn request(&self) -> &ExecutionSourceRequest {
        &self.request
    }

    pub(crate) fn root_workflow_path(&self) -> &str {
        &self.root_workflow_path
    }

    pub(crate) fn root_commit_sha(&self) -> &str {
        &self.root_commit_sha
    }

    pub(crate) fn leaf_workflow_path(&self) -> &str {
        &self.leaf_workflow_path
    }

    pub(crate) fn leaf_commit_sha(&self) -> &str {
        &self.leaf_commit_sha
    }

    pub(crate) fn rest_job_id(&self) -> Option<&str> {
        self.rest_job_id.as_deref()
    }

    pub(crate) fn canonical_json(&self) -> Result<Vec<u8>, HostError> {
        let bytes = serde_json::to_vec(self).map_err(|_| HostError::Journal)?;
        if bytes.len() > MAX_PROOF_BYTES {
            return Err(HostError::Journal);
        }
        Ok(bytes)
    }

    pub(crate) fn validate(&self) -> Result<(), HostError> {
        if !self.request.valid()
            || !valid_digest(&self.request_digest)
            || self.request.sha256()? != self.request_digest
            || !valid_git_sha(&self.tested_commit_sha)
            || !valid_relative_path(&self.root_workflow_path)
            || !valid_git_sha(&self.root_commit_sha)
            || !valid_git_sha(&self.root_tree_sha)
            || !valid_git_sha(&self.root_blob_sha)
            || !valid_digest(&self.root_content_sha256)
            || !valid_relative_path(&self.leaf_workflow_path)
            || !valid_git_sha(&self.leaf_commit_sha)
            || !valid_git_sha(&self.leaf_tree_sha)
            || !valid_git_sha(&self.leaf_blob_sha)
            || !valid_digest(&self.leaf_content_sha256)
            || self.workflow_closure.is_empty()
            || self.workflow_closure.len() > MAX_CLOSURE_FILES
            || self.workflow_closure.iter().any(|file| !file.valid())
            || has_duplicate_workflow_paths(&self.workflow_closure)
            || !self.request_matches_tested_source()
            || !self.request_root_ref_matches()
            || self.workflow_ref_event_ref() != Some(self.request.event_ref())
            || self.request.workflow_sha() != self.root_commit_sha
            || self.root_path_count() != 1
            || self.leaf_path_count() != 1
            || !optional_text(self.rest_job_id.as_deref(), 128)
        {
            return Err(HostError::Identity);
        }
        self.canonical_json()?;
        Ok(())
    }

    pub(crate) fn request_matches_tested_source(&self) -> bool {
        if self.request.tested_sha() != self.tested_commit_sha {
            return false;
        }
        match workflow_event(self.request.event_name()) {
            Some(WorkflowEvent::PullRequest) => {
                let Some(source) = self.pull_request_source.as_ref() else {
                    return false;
                };
                source.valid()
                    && self.request.pull_request_number() == Some(source.number)
                    && u64::try_from(source.base_repository_id).ok()
                        == Some(self.request.repository_id())
                    && source.base_repository_name == self.request.repository_name()
                    && self.request.base_ref() == Some(source.base_ref.as_str())
                    && self.request.base_sha() == Some(source.base_sha.as_str())
                    && self.request.head_ref() == Some(source.head_ref.as_str())
                    && self.request.head_sha() == Some(source.head_sha.as_str())
                    && self.request.event_ref() == source.merge_ref
                    && source.merge_commit_sha == self.tested_commit_sha
            }
            Some(_) => {
                self.pull_request_source.is_none()
                    && self.request.pull_request_number().is_none()
                    && self.request.base_ref().is_none()
                    && self.request.base_sha().is_none()
                    && self.request.head_ref().is_none()
                    && self.request.head_sha().is_none()
            }
            None => false,
        }
    }

    fn policy_role(
        &self,
        policy: &ApprovedJobAuthorityPolicy,
        repository: &RepositoryConnection,
    ) -> Option<JobAuthorityRole> {
        if self.validate().is_err()
            || policy.validate_for(repository).is_err()
            || policy.connection_id() != &repository.connection_id
            || self.request.repository_id() != u64::try_from(repository.repository_id).ok()?
            || self.request.repository_name() != repository.repository
            || !self.request_root_ref_matches()
        {
            return None;
        }
        for file in &self.workflow_closure {
            let repository_id = i64::try_from(file.repository_id).ok()?;
            if policy.workflow_content_sha256(repository_id, file.path())
                != Some(file.content_sha256())
            {
                return None;
            }
        }
        let event = workflow_event(self.request.event_name())?;
        if self.workflow_ref_event_ref() != Some(self.request.event_ref()) {
            return None;
        }
        let pull_request = self
            .pull_request_source
            .as_ref()
            .map(PersistedPullRequestSource::policy_rule);
        let lookup = JobAuthorityLookup {
            root_path: &self.root_workflow_path,
            workflow_path: &self.leaf_workflow_path,
            job_key: self.request.job_key(),
            event,
            event_ref: if event == WorkflowEvent::PullRequest {
                ""
            } else {
                self.request.event_ref()
            },
            pull_request: pull_request.as_ref(),
        };
        policy
            .entry(&lookup)
            .map(crate::config::ApprovedJobAuthorityEntry::role)
    }

    /// Recompute the role recorded for this persisted row during schema
    /// validation. This does not establish a live source-verifier result.
    pub(crate) fn recorded_policy_role(
        &self,
        policy: &ApprovedJobAuthorityPolicy,
        repository: &RepositoryConnection,
    ) -> Option<JobAuthorityRole> {
        self.policy_role(policy, repository)
    }

    fn root_path_count(&self) -> usize {
        self.workflow_closure
            .iter()
            .filter(|file| {
                file.repository_id == self.request.repository_id()
                    && file.path == self.root_workflow_path
                    && file.commit_sha == self.root_commit_sha
                    && file.tree_sha == self.root_tree_sha
                    && file.blob_sha == self.root_blob_sha
                    && file.content_sha256 == self.root_content_sha256
            })
            .count()
    }

    fn leaf_path_count(&self) -> usize {
        self.workflow_closure
            .iter()
            .filter(|file| {
                file.repository_id == self.request.repository_id()
                    && file.path == self.leaf_workflow_path
                    && file.commit_sha == self.leaf_commit_sha
                    && file.tree_sha == self.leaf_tree_sha
                    && file.blob_sha == self.leaf_blob_sha
                    && file.content_sha256 == self.leaf_content_sha256
            })
            .count()
    }

    fn request_root_ref_matches(&self) -> bool {
        let Some((owner, remainder)) = self.request.workflow_ref().split_once('/') else {
            return false;
        };
        let Some((repository_name, path_and_ref)) = remainder.split_once('/') else {
            return false;
        };
        if owner.is_empty()
            || repository_name.is_empty()
            || self.request.repository_name() != format!("{owner}/{repository_name}")
        {
            return false;
        }
        let Some((path, reference)) = path_and_ref.rsplit_once('@') else {
            return false;
        };
        path == self.root_workflow_path && !reference.is_empty()
    }

    fn workflow_ref_event_ref(&self) -> Option<&str> {
        self.request
            .workflow_ref()
            .rsplit_once('@')
            .map(|(_, reference)| reference)
    }
}

impl AuthorizedExecutionSource {
    pub(crate) fn matches_challenge(&self, challenge: &ExecutionProofChallenge) -> bool {
        self.request.challenge() == challenge.challenge()
            && self.request.reservation_id() == challenge.reservation_id()
            && self.request.intent_id() == challenge.intent_id()
            && self.channel_digest() == challenge.channel_digest()
    }

    pub(crate) fn matches_channel(&self, channel: &VerifiedProofChannelBinding) -> bool {
        self.request.reservation_id() == channel.reservation_id()
            && self.request.intent_id() == channel.operation_id()
            && self.channel_digest() == channel.channel_digest()
    }

    /// Match only a live source-verifier result to configured job authority.
    /// Persisted source JSON cannot call this approval operation.
    pub(crate) fn approved_role(
        &self,
        policy: &ApprovedJobAuthorityPolicy,
        repository: &RepositoryConnection,
    ) -> Option<JobAuthorityRole> {
        self.record.policy_role(policy, repository)
    }
}

fn has_duplicate_workflow_paths(files: &[VerifiedWorkflowIdentity]) -> bool {
    for (index, file) in files.iter().enumerate() {
        if files[index + 1..]
            .iter()
            .any(|other| file.repository_id == other.repository_id && file.path == other.path)
        {
            return true;
        }
    }
    false
}

fn workflow_event(value: &str) -> Option<WorkflowEvent> {
    match value {
        "pull_request" => Some(WorkflowEvent::PullRequest),
        "push" => Some(WorkflowEvent::Push),
        "merge_group" => Some(WorkflowEvent::MergeGroup),
        "schedule" => Some(WorkflowEvent::Schedule),
        "workflow_dispatch" => Some(WorkflowEvent::WorkflowDispatch),
        "release" => Some(WorkflowEvent::Release),
        _ => None,
    }
}

fn valid_repository_name(value: &str) -> bool {
    let Some((owner, repository)) = value.split_once('/') else {
        return false;
    };
    let valid_part = |part: &str| {
        !part.is_empty()
            && part.len() <= 100
            && part
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
    };
    valid_part(owner) && valid_part(repository) && !repository.contains('/')
}
