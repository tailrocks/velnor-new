//! Keep one journal, engine, host budget, and live config view.

use std::path::Path;

use crate::config::HostConfig;
use crate::daemon_lock::DaemonLock;
use crate::error::HostError;
use crate::launch::ReleaseReadSecret;
use crate::launch_blocking::ListenFault;

#[path = "loop/connection_poll.rs"]
mod connection_poll;
#[path = "loop/controller.rs"]
mod controller;
#[path = "loop/polling.rs"]
mod polling;
#[path = "loop/setup.rs"]
mod setup;
#[path = "loop/state.rs"]
mod state;
#[cfg(test)]
#[path = "loop/successor_recovery_tests.rs"]
mod successor_recovery_tests;
#[path = "loop/supervision.rs"]
mod supervision;
#[path = "loop/transitions.rs"]
mod transitions;

#[cfg(test)]
pub(crate) use connection_poll::fairness::{
    BindingDisposition, DeleteReadiness, ExecutionDisposition,
};
#[cfg(test)]
pub(crate) use connection_poll::fairness::{FairnessTurn, HeaderPollPlan};
use controller::run_controller;
#[cfg(test)]
pub(crate) use polling::PollKind;
pub(crate) use setup::connection_pat;
#[cfg(test)]
pub(crate) use setup::{requested_header_capacity, requested_header_capacity_after};
#[cfg(test)]
pub(crate) use supervision::{LocalWorkerHealthSupervisor, supervise_local_worker_health};
#[cfg(test)]
pub(crate) use transitions::{TransitionEffects, apply_config_transition, retire_pending};

/// Run the shared controller without acquiring `daemon.lock`.
///
/// The caller keeps the daemon lock for this function's full run. Only one
/// controller run may use the same held lock handle at a time.
///
/// # Errors
///
/// Returns [`ListenFault`] if the selected engine, journal, or local controller fails to start.
pub fn run_host_blocking(
    config: &HostConfig,
    state: &Path,
    daemon_lock: &DaemonLock,
) -> Result<(), ListenFault> {
    let _controller_run = daemon_lock.begin_controller_run()?;
    let state_root = std::fs::canonicalize(state).map_err(|_| HostError::Path)?;
    if !state_root.is_dir() {
        return Err(HostError::Path.into());
    }
    if !daemon_lock.matches_path(&state_root.join("daemon.lock")) {
        return Err(HostError::Lock.into());
    }
    let release_read_secret = ReleaseReadSecret::load(config)?;
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|_| HostError::Journal)?;
    runtime.block_on(run_controller(
        config.clone(),
        state,
        daemon_lock,
        release_read_secret,
    ))
}

struct PendingAcquiredRecovery {
    reservation_id: i64,
    intent_id: i64,
    failure: HostError,
}
