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

/// Conservative trust scope when only the event is known.
///
/// An event name cannot establish branch protection, default-branch identity,
/// or repository source authority. Qualified runner context may establish
/// `Trusted` separately; event-only callers always remain read-only.
#[must_use]
pub const fn trust_for_event(_event: WorkflowEvent) -> Trust {
    Trust::Pr
}
