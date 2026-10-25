use super::plan::WorkflowEvent;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Trust {
    Trusted,

    Pr,
}

#[must_use]
pub const fn trust_for_event(event: WorkflowEvent) -> Trust {
    match event {
        WorkflowEvent::Push => Trust::Trusted,
        WorkflowEvent::PullRequest
        | WorkflowEvent::Fork
        | WorkflowEvent::Local
        | WorkflowEvent::MergeGroup
        | WorkflowEvent::Qualification => Trust::Pr,
    }
}
