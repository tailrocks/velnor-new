//! Event trust scopes: canonical `Trust` per `WorkflowEvent`.
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

/// Canonical trust scope for one triggering event.
///
/// Only branch pushes run protected-branch content; pull requests,
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
