//! Host effects: Docker ownership, launchd, and local IPC.
//!
//! Durable intent lives in velnor-runner-journal and is re-exported here.

pub mod assign;
mod config;
mod config_file;
mod connect;
mod daemon_lock;
pub mod docker_client;
pub mod guest;
mod https;
mod ipc;
mod keychain;
pub mod listen;
mod plist;
mod readiness;
pub mod scale_set;
pub mod stage;
pub mod worker;

pub use assign::{Offer, offer};
pub use config::{
    DockerConfig, GithubSection, HostConfig, HostLimits, HostPlatform, JobTrustPolicy,
    RegistrationScopeKind, RunnerConfig, ScaleSetBinding,
};
pub use config_file::{
    LINUX_CONFIG_PATH, linux_service_group_id, persist_host_config_file, read_host_config_file,
    remove_host_config_file, validate_host_config_target,
};
pub use connect::{ConnectPlan, DisconnectEffect, SetOwnership, connect_plan, disconnect_effects};
pub use daemon_lock::DaemonLock;
pub use docker_client::connect_unix;
pub use guest::guest_slots;
pub use https::HttpsTransport;
pub use ipc::{MAX_FRAME, SOCKET_DIR_MODE, decode_frame, encode_frame};
pub use keychain::{
    import_secret, load_configured_secret, load_secret, read_secret, remove_configured_secret,
    store_configured_secret,
};
pub use listen::{SessionCensus, SessionProbe, probe_once, queue_path, session_census};
pub use plist::{keychain_import_argv, launch_agent_plist};
pub use readiness::{Readiness, doctor_json, readiness_for_empty, status_json};
pub use scale_set::{
    EnsureError, EnsuredSet, discover_product_scale_set, ensure_product_scale_set,
    ensure_product_scale_set_for_binding, product_runner_groups,
};
pub use stage::{
    PairStop, PartialPair, drive_with_profile, remove_recorded, start_pair_until,
    start_pair_until_with_profile,
};
pub use velnor_runner_docker_spec::{
    ContainerPlan, DeleteDecision, RunnerImageProfile, audit_plan, delete_decision, plan_contains,
    resolve_runner_profile, runner_plan, runner_plan_for_profile,
};
pub use velnor_runner_journal::{
    HostError, IntentRow, IntentState, Journal, Outcome, Reconcile, ReleaseFact, before_advertise,
    occupies, release_permitted,
};
pub use worker::{
    BollardCreate, CreateProjection, Started, bollard_create, dind_create, dind_create_for_profile,
    runner_create, start_pair, start_pair_with_profile,
};
