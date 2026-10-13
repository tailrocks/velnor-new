//! Read-only exact-attempt Actions REST inventory for provider comparisons.
//!
//! The result is raw provider evidence, not a lane mapper or parity proof.

mod api;
mod read;
mod types;
mod validate;

pub use api::read_actions_workflow_attempt_provider_evidence_async;
pub use types::{
    ActionsWorkflowAttemptEvidenceGap, ActionsWorkflowAttemptJobEvidence,
    ActionsWorkflowAttemptProviderEvidence, ActionsWorkflowAttemptProviderRead,
    ActionsWorkflowRunArtifactEvidence,
};
