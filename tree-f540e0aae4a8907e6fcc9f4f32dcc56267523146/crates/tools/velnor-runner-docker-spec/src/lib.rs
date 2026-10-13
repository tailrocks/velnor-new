//! Audited runner container plans and runner image profiles.
//!
//! Pure derivation: container plans, mount auditing, and delete decisions
//! over journal errors. Host orchestration lives in velnor-runner-host.

mod docker_spec;

pub use docker_spec::{
    ContainerPlan, DeleteDecision, ImageMount, Mount, RunnerImageProfile, audit_plan,
    delete_decision, plan_contains, resolve_linux_admission_profile, resolve_runner_profile,
    runner_plan, runner_plan_for_profile, validate_linux_admission_profile,
};
pub use velnor_runner_journal::HostError;
