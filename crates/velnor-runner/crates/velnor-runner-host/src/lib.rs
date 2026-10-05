//! Host effects: journal, Docker ownership, launchd, and local IPC.
//!
//! Callers persist intent before an external effect and never hold a
//! transaction across that effect.

mod assign;
mod config;
mod connect;
mod daemon_lock;
mod docker_client;
mod docker_spec;
mod error;
mod guest;
mod https;
mod ipc;
mod journal;
mod keychain;
mod launch;
mod launch_blocking;
mod listen;
mod plist;
mod readiness;
mod reconcile;
mod scale_set;
mod stage;
mod worker;

pub use assign::{Offer, offer};
pub use config::{DockerConfig, GithubSection, HostConfig, HostLimits};
pub use connect::{ConnectPlan, DisconnectEffect, SetOwnership, connect_plan, disconnect_effects};
pub use daemon_lock::DaemonLock;
pub use docker_client::connect_unix;
pub use docker_spec::{
    ContainerPlan, DeleteDecision, audit_plan, delete_decision, plan_contains, runner_plan,
};
pub use error::HostError;
pub use guest::guest_slots;
pub use https::HttpsTransport;
pub use ipc::{MAX_FRAME, SOCKET_DIR_MODE, decode_frame, encode_frame};
pub use journal::{IntentState, Journal, Outcome};
pub use keychain::{import_secret, load_secret, read_secret};
pub use launch::{LaunchReport, launch_once};
pub use launch_blocking::{ListenFault, launch_blocking};
pub use listen::{SessionCensus, SessionProbe, probe_once, queue_path, session_census};
pub use plist::{keychain_import_argv, launch_agent_plist};
pub use readiness::{Readiness, doctor_json, readiness_for_empty, status_json};
pub use reconcile::{
    IntentRow, Reconcile, ReleaseFact, before_advertise, occupies, release_permitted,
};
pub use scale_set::{EnsureError, EnsuredSet, ensure_product_scale_set, product_runner_groups};
pub use stage::{PairStop, PartialPair, remove_recorded, start_pair_until};
pub use worker::{
    BollardCreate, CreateProjection, Started, bollard_create, dind_create, runner_create,
    start_pair,
};

#[cfg(test)]
mod assign_tests;
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
mod journal_tests;
#[cfg(test)]
mod journal_worker_volume_tests;
#[cfg(test)]
mod keychain_tests;
#[cfg(test)]
mod launch_backfill_tests;
#[cfg(test)]
mod launch_capacity_tests;
#[cfg(test)]
mod launch_harness;
#[cfg(test)]
mod launch_idless_tests;
#[cfg(test)]
mod launch_scale_tests;
#[cfg(test)]
mod launch_test_support;
#[cfg(test)]
mod launch_tests;
#[cfg(test)]
mod launch_worker_cleanup_tests;
#[cfg(test)]
mod listen_tests;
#[cfg(test)]
mod plist_tests;
#[cfg(test)]
mod readiness_tests;
#[cfg(test)]
mod runner_image_contract_tests;
#[cfg(test)]
mod stage_tests;
#[cfg(test)]
mod worker_tests;
