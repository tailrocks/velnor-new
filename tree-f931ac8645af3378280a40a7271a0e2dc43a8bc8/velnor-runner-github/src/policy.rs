//! Typed, fail-closed evidence for runner-pool and per-message trust.
//!
//! Pool routing proof and per-message trust are separate. A verified message
//! does not authorize acquisition or minting, and a pool proof never binds an
//! acquired request to a particular JIT runner.

mod job;
mod pool;
mod types;
mod wire;

pub use job::{JobTrustEvidence, VerifiedJobTrust, verify_job_offer};
pub use pool::{PoolAdmissionEvidence, VerifiedPoolPolicy, verify_pool_policy};
pub use types::{
    ActionsWorkflowTrustRun, JobTrustPolicyView, JobTrustRuleView, PolicyGap, PolicyMismatch,
    PoolBinding, PoolBindingView, PoolEvidenceSource, PoolEvidenceSourceStamp, PoolPolicySnapshot,
    ReusableWorkflowEvidence, ReusableWorkflowRuleView, WorkflowTrustField,
    get_actions_workflow_trust_run, workflow_ref_for_repository,
};
pub use wire::{ParsedTrustBatch, ParsedTrustEvent, PollWithTrust, parse_poll_with_trust};

#[cfg(test)]
mod tests;
