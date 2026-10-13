//! Host effects: journal, Docker ownership, launchd, and local IPC.
//!
//! Callers persist intent before an external effect and never hold a
//! transaction across that effect.

mod config;
mod connect;
mod daemon_lock;
mod docker_client;
mod docker_spec;
mod error;
mod ipc;
mod journal;
mod plist;
mod readiness;

pub use config::{DockerConfig, GithubSection, HostConfig, HostLimits};
pub use connect::{ConnectPlan, DisconnectEffect, SetOwnership, connect_plan, disconnect_effects};
pub use daemon_lock::DaemonLock;
pub use docker_client::connect_unix;
pub use docker_spec::{
    ContainerPlan, DeleteDecision, audit_plan, delete_decision, plan_contains, runner_plan,
};
pub use error::HostError;
pub use ipc::{MAX_FRAME, SOCKET_DIR_MODE, decode_frame, encode_frame};
pub use journal::{IntentState, Journal, Outcome};
pub use plist::{keychain_import_argv, launch_agent_plist};
pub use readiness::{Readiness, doctor_json, readiness_for_empty, status_json};

#[cfg(test)]
mod connect_tests;
#[cfg(test)]
mod docker_client_tests;
#[cfg(test)]
mod docker_spec_tests;
#[cfg(test)]
mod ipc_tests;
#[cfg(test)]
mod journal_tests;
#[cfg(test)]
mod plist_tests;
#[cfg(test)]
mod readiness_tests;
