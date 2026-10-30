use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use velnor_runner_github::workflow_run::{GitCommit, PullRequestBaseHistory, ordered_base_changes};

use super::plan::TransitionPlan;
use super::types::TransitionError;
use super::validation::{DIGEST_HEX_LEN, hex, valid_hex, valid_opaque_id, valid_sha};

const MAX_HISTORY_EVENTS: usize = 6400;

/// Bounded identity fields returned by the pull request update response.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct RetargetResponseReceipt {
    pub(crate) pull_request_number: i64,
    pub(crate) base_repository_id: i64,
    pub(crate) head_repository_id: i64,
    pub(crate) head_ref: String,
    pub(crate) head_sha: String,
    pub(crate) previous_base_ref: String,
    pub(crate) previous_base_sha: String,
    pub(crate) current_base_ref: String,
    pub(crate) current_base_sha: String,
}

impl RetargetResponseReceipt {
    pub(crate) fn matches(&self, plan: &TransitionPlan) -> bool {
        self.pull_request_number == plan.pull_request_number
            && self.base_repository_id == plan.repository_id
            && self.head_repository_id == plan.head_repository_id
            && self.head_ref == plan.head_ref
            && self.head_sha == plan.head_sha
            && self.previous_base_ref == plan.previous_base_ref
            && self.previous_base_sha == plan.previous_base_sha
            && self.current_base_ref == plan.target_base_ref
            && valid_sha(&self.current_base_sha)
    }
}

/// Exact post-update snapshot and completed timeline proof.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct RetargetHistoryReceipt {
    pub(crate) pull_request_number: i64,
    pub(crate) base_repository_id: i64,
    pub(crate) head_repository_id: i64,
    pub(crate) head_ref: String,
    pub(crate) head_sha: String,
    pub(crate) base_ref: String,
    pub(crate) base_sha: String,
    pub(crate) event_id: String,
    pub(crate) event_created_at: String,
    pub(crate) event_previous_ref: String,
    pub(crate) event_current_ref: String,
    pub(crate) previous_history_digest: String,
    pub(crate) previous_event_count: u32,
    pub(crate) complete_history_digest: String,
    pub(crate) complete_event_count: u32,
}

impl RetargetHistoryReceipt {
    /// Build proof only when the complete history adds this one exact final base change.
    pub(crate) fn from_history(
        history: &PullRequestBaseHistory,
        plan: &TransitionPlan,
        response: &RetargetResponseReceipt,
    ) -> Result<Self, TransitionError> {
        if !response.matches(plan)
            || history.number != plan.pull_request_number
            || history.repository_id != plan.repository_id
            || history.head_repository_id != plan.head_repository_id
            || history.base_repository_id != plan.repository_id
            || history.head_ref != plan.head_ref
            || history.head_sha != plan.head_sha
            || history.base_ref != plan.target_base_ref
            || history.base_sha != response.current_base_sha
        {
            return Err(TransitionError::IncorrectHistory);
        }

        let ordered =
            ordered_base_changes(history).map_err(|_| TransitionError::IncorrectHistory)?;
        let prior_count = usize::try_from(plan.base_history_event_count)
            .map_err(|_| TransitionError::IncorrectHistory)?;
        let complete_count = prior_count
            .checked_add(1)
            .ok_or(TransitionError::IncorrectHistory)?;
        if ordered.len() != complete_count || ordered.len() > MAX_HISTORY_EVENTS {
            return Err(TransitionError::IncorrectHistory);
        }
        let previous_history_digest = digest_events(&ordered[..prior_count]);
        if previous_history_digest != plan.base_history_digest {
            return Err(TransitionError::IncorrectHistory);
        }
        let event = ordered.last().ok_or(TransitionError::IncorrectHistory)?;
        if event.previous_ref_name != plan.previous_base_ref
            || event.current_ref_name != plan.target_base_ref
            || !valid_opaque_id(&event.id)
        {
            return Err(TransitionError::IncorrectHistory);
        }
        let complete_event_count =
            u32::try_from(ordered.len()).map_err(|_| TransitionError::IncorrectHistory)?;
        Ok(Self {
            pull_request_number: history.number,
            base_repository_id: history.base_repository_id,
            head_repository_id: history.head_repository_id,
            head_ref: history.head_ref.clone(),
            head_sha: history.head_sha.clone(),
            base_ref: history.base_ref.clone(),
            base_sha: history.base_sha.clone(),
            event_id: event.id.clone(),
            event_created_at: event.created_at.clone(),
            event_previous_ref: event.previous_ref_name.clone(),
            event_current_ref: event.current_ref_name.clone(),
            previous_history_digest,
            previous_event_count: plan.base_history_event_count,
            complete_history_digest: digest_events(&ordered),
            complete_event_count,
        })
    }

    pub(crate) fn matches(
        &self,
        plan: &TransitionPlan,
        response: &RetargetResponseReceipt,
    ) -> bool {
        response.matches(plan)
            && self.pull_request_number == plan.pull_request_number
            && self.base_repository_id == plan.repository_id
            && self.head_repository_id == plan.head_repository_id
            && self.head_ref == plan.head_ref
            && self.head_sha == plan.head_sha
            && self.base_ref == plan.target_base_ref
            && self.base_sha == response.current_base_sha
            && valid_opaque_id(&self.event_id)
            && !self.event_created_at.is_empty()
            && self.event_previous_ref == plan.previous_base_ref
            && self.event_current_ref == plan.target_base_ref
            && self.previous_history_digest == plan.base_history_digest
            && self.previous_event_count == plan.base_history_event_count
            && valid_hex(&self.complete_history_digest, DIGEST_HEX_LEN)
            && self.complete_event_count == self.previous_event_count.saturating_add(1)
    }
}

/// Fingerprint an authenticated complete base-ref history for the initial plan.
pub(crate) fn base_history_fingerprint(
    history: &PullRequestBaseHistory,
) -> Result<(u32, String), TransitionError> {
    let ordered = ordered_base_changes(history).map_err(|_| TransitionError::IncorrectHistory)?;
    if ordered.len() > MAX_HISTORY_EVENTS {
        return Err(TransitionError::IncorrectHistory);
    }
    let count = u32::try_from(ordered.len()).map_err(|_| TransitionError::IncorrectHistory)?;
    Ok((count, digest_events(&ordered)))
}

fn digest_events(events: &[velnor_runner_github::workflow_run::BaseRefChangedEvent]) -> String {
    let mut digest = Sha256::new();
    digest.update(b"velnor-pr-base-history-v1\0");
    digest.update(
        u64::try_from(events.len())
            .unwrap_or(u64::MAX)
            .to_le_bytes(),
    );
    for event in events {
        hash_text(&mut digest, &event.id);
        match event.database_id {
            Some(id) => {
                digest.update([1]);
                digest.update(id.to_le_bytes());
            }
            None => digest.update([0]),
        }
        hash_text(&mut digest, &event.created_at);
        hash_text(&mut digest, &event.previous_ref_name);
        hash_text(&mut digest, &event.current_ref_name);
    }
    hex(&digest.finalize())
}

fn hash_text(hash: &mut Sha256, value: &str) {
    hash.update(u64::try_from(value.len()).unwrap_or(u64::MAX).to_le_bytes());
    hash.update(value.as_bytes());
}

/// Exact commit-object response facts without storing an untrusted commit message body.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct NonceCommitReceipt {
    pub(crate) sha: String,
    pub(crate) tree_sha: String,
    pub(crate) parent_sha: String,
    pub(crate) message_sha256: String,
    pub(crate) nonce_marker_count: u8,
    pub(crate) sign_off_count: u8,
    pub(crate) author_name: String,
    pub(crate) author_email: String,
    pub(crate) committer_name: String,
    pub(crate) committer_email: String,
}

impl NonceCommitReceipt {
    pub(crate) fn from_commit(
        commit: &GitCommit,
        plan: &TransitionPlan,
        nonce: &str,
    ) -> Option<Self> {
        let author = commit.author.as_ref()?;
        let committer = commit.committer.as_ref()?;
        let [parent] = commit.parents.as_slice() else {
            return None;
        };
        let nonce_marker = format!("velnor-pr-validation-nonce:v1:{nonce}");
        let sign_off = plan.commit_identity.sign_off_line();
        let nonce_marker_count = line_count(&commit.message, &nonce_marker);
        let sign_off_count = line_count(&commit.message, &sign_off);
        if !valid_sha(&commit.sha)
            || !valid_sha(&commit.tree.sha)
            || parent.sha != plan.head_sha
            || author.name != plan.commit_identity.name()
            || author.email != plan.commit_identity.email()
            || committer.name != plan.commit_identity.name()
            || committer.email != plan.commit_identity.email()
            || nonce_marker_count != 1
            || sign_off_count != 1
        {
            return None;
        }
        Some(Self {
            sha: commit.sha.clone(),
            tree_sha: commit.tree.sha.clone(),
            parent_sha: parent.sha.clone(),
            message_sha256: message_digest(&commit.message),
            nonce_marker_count,
            sign_off_count,
            author_name: author.name.clone(),
            author_email: author.email.clone(),
            committer_name: committer.name.clone(),
            committer_email: committer.email.clone(),
        })
    }
}

/// Bounded identity facts retained when a commit response cannot be accepted.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct NonceCommitObservation {
    pub(crate) sha: String,
    pub(crate) tree_sha: String,
    pub(crate) parent_count: u32,
    pub(crate) parents_sha256: String,
    pub(crate) message_sha256: String,
    pub(crate) author: Option<CommitPersonObservation>,
    pub(crate) committer: Option<CommitPersonObservation>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct CommitPersonObservation {
    pub(crate) name_sha256: String,
    pub(crate) email_sha256: String,
}

impl NonceCommitObservation {
    pub(crate) fn from_commit(commit: &GitCommit) -> Result<Self, TransitionError> {
        let parent_count =
            u32::try_from(commit.parents.len()).map_err(|_| TransitionError::IncorrectHistory)?;
        let mut parents = Sha256::new();
        parents.update(b"velnor-pr-commit-parents-v1\0");
        parents.update(
            u64::try_from(commit.parents.len())
                .unwrap_or(u64::MAX)
                .to_le_bytes(),
        );
        for parent in &commit.parents {
            hash_text(&mut parents, &parent.sha);
        }
        Ok(Self {
            sha: commit.sha.clone(),
            tree_sha: commit.tree.sha.clone(),
            parent_count,
            parents_sha256: hex(&parents.finalize()),
            message_sha256: message_digest(&commit.message),
            author: commit
                .author
                .as_ref()
                .map(CommitPersonObservation::from_person),
            committer: commit
                .committer
                .as_ref()
                .map(CommitPersonObservation::from_person),
        })
    }
}

impl CommitPersonObservation {
    fn from_person(person: &velnor_runner_github::workflow_run::GitCommitPerson) -> Self {
        Self {
            name_sha256: text_digest(b"velnor-pr-commit-name-v1\0", &person.name),
            email_sha256: text_digest(b"velnor-pr-commit-email-v1\0", &person.email),
        }
    }
}

/// Exact Git reference state observed after the one non-force publication request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct ReferenceReceipt {
    pub(crate) reference: String,
    pub(crate) object_type: String,
    pub(crate) sha: String,
}

fn line_count(message: &str, expected: &str) -> u8 {
    message
        .lines()
        .filter(|line| *line == expected)
        .count()
        .min(usize::from(u8::MAX)) as u8
}

fn message_digest(message: &str) -> String {
    text_digest(b"velnor-pr-commit-message-v1\0", message)
}

fn text_digest(prefix: &[u8], value: &str) -> String {
    let mut digest = Sha256::new();
    digest.update(prefix);
    hash_text(&mut digest, value);
    hex(&digest.finalize())
}
