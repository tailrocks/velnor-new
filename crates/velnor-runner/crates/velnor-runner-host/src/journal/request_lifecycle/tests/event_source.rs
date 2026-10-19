//! Native event-source validation and restart-safe correlation regressions.

use super::helpers::{
    ProvisionedRunner, Scratch, binding, created_scope, event_source, provision_physical_intent,
    receipt, record_acquired, record_owned_storage_cleanup, runner_name, settle_storage_metadata,
};
use crate::error::HostError;
use crate::journal::{
    AssignmentIdentity, AssignmentKey, CapacityLimits, ConnectionJournal, Journal, ReservationState,
};
use velnor_runner_github::{RunnerReference, SourceMetadata};

struct StartedFixture {
    binding: crate::config::RepositoryConnection,
    scope: ConnectionJournal,
    physical: AssignmentIdentity,
    intent_id: i64,
    request: AssignmentIdentity,
    source: velnor_runner_github::SourceMetadata,
    runner: RunnerReference,
    provisioned: ProvisionedRunner,
}

include!("event_source/event_cases.rs");
include!("event_source/started_helpers.rs");
include!("event_source/runnerless_case.rs");
