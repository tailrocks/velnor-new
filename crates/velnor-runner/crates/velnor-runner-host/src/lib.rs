//! Host effects: journal, Docker ownership, launchd, and local IPC.
//!
//! Callers persist intent before an external effect and never hold a
//! transaction across that effect.

pub(crate) mod artifact_admission;
mod assign;
mod compile_identity;
mod config;
mod connect;
mod controller_ipc;
mod daemon_lock;
mod digest_hex;
mod docker_client;
mod docker_spec;
mod error;
mod helper_supervisor;
mod host_controller;
mod https;
mod ipc;
mod journal;
mod journal_assignment;
mod journal_identity;
mod journal_schema;
mod journal_sql;
mod keychain;
mod launch;
mod launch_blocking;
mod launch_identity;
mod listen;
mod plist;
mod readiness;
mod readiness_scan;
mod reconcile;
pub(crate) mod resource_probe_record;
pub mod resource_probe_release;
mod scale_set;
mod stage;
mod worker;

pub use assign::{Offer, offer};
pub use config::{
    ConnectionId, DockerConfig, HostConfig, HostLimits, KeychainReference, PullRequestRule,
    RepositoryConnection, ScaleSetSelector, SourcePolicy, SourcePolicyMode, WorkflowEvent,
};
pub use connect::{ConnectPlan, DisconnectEffect, SetOwnership, connect_plan, disconnect_effects};
pub use controller_ipc::{
    ConnectionCensus, ConnectionSetupReceipt, RunnerGroupReceipt, SelectedSetReceipt,
    SessionObservation, SessionObservationStatistics, controller_readiness_status,
    reactivate_connection_if_drained, run_doctor_probe, wait_for_connection_census,
    wait_for_connection_detached, wait_for_connection_drain, wait_for_connection_setup_receipt,
    wait_for_drain, wait_for_session_observation,
};
pub use daemon_lock::DaemonLock;
pub use docker_client::connect_unix;
pub use docker_spec::{
    ContainerPlan, DeleteDecision, audit_plan, delete_decision, plan_contains, runner_plan,
};
pub use error::{HostError, PreparationCause};
pub use host_controller::run_host_blocking;
pub use https::HttpsTransport;
pub use ipc::{MAX_FRAME, SOCKET_DIR_MODE, decode_frame, encode_frame};
pub use journal::pr_source_transition::{
    EffectClaim, PrSourceTransitionJournal, TransitionEffect, TransitionError, TransitionPlan,
    TransitionPlanInput, TransitionRecovery, TransitionResponseDisposition, TransitionSnapshot,
    TransitionStage, UncertaintyReason,
};
pub use journal::{IntentState, Journal, Outcome};
pub use keychain::{import_secret, load_secret, read_secret};
pub use launch_blocking::ListenFault;
pub use listen::queue_path;
pub use plist::{keychain_import_argv, launch_agent_plist};
pub use readiness::{Readiness, doctor_json, readiness_for_empty, status_json};
pub use readiness_scan::{READINESS_BUDGET, controller_readiness};
pub use reconcile::{
    IntentRow, LaunchPhase, Reconcile, ReleaseFact, before_advertise, occupies, release_permitted,
};
pub use scale_set::{EnsureError, EnsuredSet, product_runner_groups};
pub use stage::remove_recorded;
pub use worker::{
    BindMount, BollardCreate, CreateProjection, ResourceBudget, Started, bollard_create,
    dind_create, runner_create,
};

#[cfg(test)]
mod assign_tests;
#[cfg(test)]
mod config_test_support;
#[cfg(test)]
mod config_tests;
#[cfg(test)]
mod connect_tests;
#[cfg(test)]
mod docker_client_tests;
#[cfg(test)]
mod docker_spec_tests;
#[cfg(test)]
mod https_tests;
#[cfg(test)]
mod ipc_tests;
#[cfg(test)]
mod journal_identity_tests;
#[cfg(test)]
mod journal_launch_phase_tests;
#[cfg(test)]
mod journal_schema_tests;
#[cfg(test)]
mod journal_tests;
#[cfg(test)]
#[path = "journal_tests_b.rs"]
mod journal_tests_b;
#[cfg(test)]
mod journal_worker_volume_tests;
#[cfg(test)]
mod keychain_tests;
#[cfg(test)]
mod launch_harness;
#[cfg(test)]
mod launch_tests;
#[cfg(test)]
mod listen_tests;
#[cfg(test)]
mod plist_tests;
#[cfg(test)]
mod readiness_tests;
#[cfg(test)]
mod reconcile_tests;
#[cfg(test)]
mod runner_image_contract_tests;
#[cfg(test)]
mod worker_tests;
