//! Cache writer intent and conservative event classification.
use super::plan::WorkflowEvent;
use serde::{Deserialize, Serialize};

/// Trust scope.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Trust {
    /// Protected default-branch scope.
    Trusted,
    /// Pull-request scope.
    Pr,
}

/// Immutable run facts from the GitHub event payload and workflow identity.
///
/// This request type deliberately contains no protection assertion and is not
/// itself authority. Mise rechecks it against the runner event source and the
/// current repository branch API before issuing an opaque writer context.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CacheWriterFacts {
    /// Resolved event kind.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub event: Option<WorkflowEvent>,
    /// Event payload ref, such as `refs/heads/main`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub git_ref: Option<String>,
    /// Event payload's repository default branch.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_branch: Option<String>,
    /// Current run repository (`owner/repo`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repository: Option<String>,
    /// Repository slug carried by the event payload.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub event_repository: Option<String>,
}

impl CacheWriterFacts {
    /// True when no event or repository facts were captured.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.event.is_none()
            && self.git_ref.is_none()
            && self.default_branch.is_none()
            && self.repository.is_none()
            && self.event_repository.is_none()
    }
}

/// Conservative trust scope when only the event type is available.
///
/// A push may target a feature branch, tag, or unprotected default branch.
/// Code that needs the trusted namespace must obtain an opaque Mise-owned
/// context after joining these payload facts to current GitHub API evidence.
#[must_use]
pub const fn trust_for_event(event: WorkflowEvent) -> Trust {
    let _ = event;
    Trust::Pr
}
