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
pub use pool::{
    PoolAdmissionEvidence, PoolAdmissionPreflight, RepositoryPoolTrustEvidence, VerifiedPoolPolicy,
    preflight_organization_pool_admission_async,
    preflight_organization_pool_admission_with_admin_async,
    preflight_pool_admission_with_admin_async, read_repository_pool_trust,
    read_repository_pool_trust_async, verify_organization_pool_policy, verify_pool_policy,
};
pub use types::{
    ActionsWorkflowTrustRun, JobTrustPolicyView, JobTrustRuleView, PolicyGap, PolicyMismatch,
    PoolBinding, PoolBindingView, PoolEvidenceSource, PoolEvidenceSourceStamp, PoolPolicySnapshot,
    PoolRegistrationScope, PoolRegistrationScopeView, ReusableWorkflowEvidence,
    ReusableWorkflowRuleView, RunnerImageIdentity, RunnerImageIdentityView, WorkflowTrustField,
    get_actions_workflow_trust_run, workflow_ref_for_repository,
};
pub use wire::{ParsedTrustBatch, ParsedTrustEvent, PollWithTrust, parse_poll_with_trust};

#[cfg(test)]
mod tests;
