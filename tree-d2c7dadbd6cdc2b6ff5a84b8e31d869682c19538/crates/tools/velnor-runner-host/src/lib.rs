//! Host effects: journal, Docker ownership, launchd, and local IPC.
//!
//! Callers persist intent before an external effect and never hold a
//! transaction across that effect.

pub mod assign;
mod config;
mod connect;
mod daemon_lock;
pub mod docker_client;
pub mod docker_spec;
mod error;
pub mod guest;
mod https;
mod ipc;
pub mod journal;
mod keychain;
pub mod listen;
mod plist;
mod readiness;
pub mod reconcile;
pub mod scale_set;
pub mod stage;
pub mod worker;

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
