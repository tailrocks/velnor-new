// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

#![allow(
    missing_docs,
    reason = "documentation comments are intentionally absent from this codebase"
)]
#![allow(
    clippy::missing_errors_doc,
    reason = "documentation comments are intentionally absent from this codebase"
)]
mod artifact_admission;
mod assign;
mod compile_identity;
mod config;
mod connect;
mod daemon_lock;
mod docker_client;
mod docker_spec;
mod error;
mod helper_supervisor;
mod https;
mod ipc;
mod journal;
mod journal_assignment;
mod journal_effects;
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
pub mod resource_probe_release;
mod scale_set;
mod stage;
mod work_owner;
mod worker;

pub use assign::{Offer, offer};
pub use config::{DockerConfig, GithubSection, HostConfig, HostLimits, KeychainReference};
pub use connect::{ConnectPlan, DisconnectEffect, SetOwnership, connect_plan, disconnect_effects};
pub use daemon_lock::DaemonLock;
pub use docker_client::connect_unix;
pub use docker_spec::{
    ContainerPlan, DeleteDecision, audit_plan, delete_decision, plan_contains, runner_plan,
};
pub use error::{HostError, PreparationCause};
pub use https::HttpsTransport;
pub use ipc::{MAX_FRAME, SOCKET_DIR_MODE, decode_frame, encode_frame};
pub use journal::{IntentState, Journal, Outcome};
pub use keychain::{import_secret, load_secret, read_secret};
pub use launch::{LaunchReport, launch_once};
pub use launch_blocking::{ListenFault, launch_blocking};
pub use listen::{SessionCensus, SessionProbe, probe_once, queue_path, session_census};
pub use plist::{keychain_import_argv, launch_agent_plist};
pub use readiness::{Readiness, doctor_json, readiness_for_empty, status_json};
pub use readiness_scan::{READINESS_BUDGET, controller_readiness};
pub use reconcile::{
    IntentRow, LaunchPhase, Reconcile, ReleaseFact, before_advertise, occupies, release_permitted,
};
pub use scale_set::{EnsureError, EnsuredSet, ensure_product_scale_set, product_runner_groups};
pub use stage::{PairStop, PartialPair, remove_recorded, start_pair_until};
pub use worker::{
    BindMount, BollardCreate, CreateProjection, ResourceBudget, Started, bollard_create,
    dind_create, runner_create, start_pair,
};
