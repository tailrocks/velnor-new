//! Event trust scopes: canonical `Trust` per `WorkflowEvent`.
use super::plan::WorkflowEvent;
use serde::{Deserialize, Serialize};

/// Immutable workflow facts required to authorize a trusted cache writer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CacheWriterContext<'a> {
    /// Event name supplied by GitHub's immutable event context.
    pub event: &'a str,
    /// Exact GitHub ref (for example `refs/heads/main`), absent if unknown.
    pub reference: Option<&'a str>,
    /// Repository default branch, absent if unavailable.
    pub default_branch: Option<&'a str>,
    /// GitHub's immutable protection fact for this ref.
    pub ref_protected: bool,
}

impl CacheWriterContext<'_> {
    /// True only for a successful-cache-eligible protected default push.
    #[must_use]
    pub fn is_protected_default_push(self) -> bool {
        self.event == "push"
            && self.ref_protected
            && self
                .default_branch
                .filter(|branch| !branch.is_empty())
                .is_some_and(|branch| {
                    self.reference
                        .and_then(|reference| reference.strip_prefix("refs/heads/"))
                        == Some(branch)
                })
    }
}

/// Trust scope.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Trust {
    /// Protected default-branch scope.
    Trusted,
    /// Pull-request scope.
    Pr,
}

/// Canonical trust scope for one triggering event.
///
/// A push marks branch content as trusted for event-level planning;
/// protected-default cache writes require [`CacheWriterContext`]. Pull requests,
/// forks, local runs, and merge-group runs all execute unreviewed or
/// speculative content under PR scope. Merge-group runs in particular
/// test speculative merges of unreviewed PRs, so `Trusted` there would
/// let PR content pollute trusted caches (consistent with baseline
/// publish staying push-only). Single source for plan stamping and
/// merge-time coherence.
#[must_use]
pub const fn trust_for_event(event: WorkflowEvent) -> Trust {
    match event {
        WorkflowEvent::Push => Trust::Trusted,
        WorkflowEvent::PullRequest
        | WorkflowEvent::Fork
        | WorkflowEvent::Local
        | WorkflowEvent::MergeGroup => Trust::Pr,
    }
}
