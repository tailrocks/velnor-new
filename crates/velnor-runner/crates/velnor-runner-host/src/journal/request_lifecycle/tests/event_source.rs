//! Native event-source validation and restart-safe correlation regressions.

use super::helpers::{
    ProvisionedRunner, Scratch, binding, created_scope, event_source,
    finish_initialized_listener_delivery, provision_acquired_request_with_initialization, receipt,
    record_owned_network_cleanup, record_owned_storage_cleanup, runner_name,
    settle_storage_metadata,
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
