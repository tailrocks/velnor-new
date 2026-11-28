// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::path::Path;

use tokio::runtime::Runtime;

use crate::config::{HostConfig, LaunchMode};
use crate::daemon_lock::DaemonLock;
use crate::error::HostError;
use crate::launch::ImageAuthority;
use crate::launch_blocking::ListenFault;
use crate::rust_runner::Handover;
use crate::shutdown_signals::Signals;

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

#[path = "loop/supervision.rs"]
mod supervision;
#[path = "loop/transitions.rs"]
mod transitions;

use controller::run_controller;

pub(crate) use setup::connection_pat;

pub fn run_host_blocking(
    config: &HostConfig,
    state: &Path,
    daemon_lock: &DaemonLock,
) -> Result<(), ListenFault> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|_| HostError::Journal)?;
    let result = serve(&runtime, config, state, daemon_lock);
    runtime.shutdown_background();
    result
}

fn serve(
    runtime: &Runtime,
    config: &HostConfig,
    state: &Path,
    daemon_lock: &DaemonLock,
) -> Result<(), ListenFault> {
    let mut signals = {
        let _entered = runtime.enter();
        Signals::install()?
    };
    let mut mode = config.host.launch_mode;
    let mut config = config.clone();
    loop {
        let handover = match mode {
            LaunchMode::OfficialRunner => {
                run_official_blocking(runtime, &config, state, daemon_lock, &mut signals)?
            }
            LaunchMode::RustRunner => {
                crate::rust_runner::run_blocking(runtime, state, daemon_lock, &mut signals)?
            }
        };
        match handover {
            Handover::Stopped => return Ok(()),
            Handover::OfficialRunner(next) => {
                mode = LaunchMode::OfficialRunner;
                config = *next;
            }
            Handover::RustRunner => mode = LaunchMode::RustRunner,
        }
    }
}

fn run_official_blocking(
    runtime: &Runtime,
    config: &HostConfig,
    state: &Path,
    daemon_lock: &DaemonLock,
    signals: &mut Signals,
) -> Result<Handover, ListenFault> {
    let _controller_run = daemon_lock.begin_controller_run()?;
    let state_root = std::fs::canonicalize(state).map_err(|_| HostError::Path)?;
    if !state_root.is_dir() {
        return Err(HostError::Path.into());
    }
    if !daemon_lock.matches_path(&state_root.join("daemon.lock")) {
        return Err(HostError::Lock.into());
    }
    let image_authority = ImageAuthority::load(config)?;
    runtime.block_on(async {
        tokio::select! {
            biased;
            () = signals.received() => Ok(Handover::Stopped),
            handover = run_controller(config.clone(), state, daemon_lock, image_authority) => handover,
        }
    })
}

struct PendingAcquiredRecovery {
    reservation_id: i64,
    intent_id: i64,
    failure: HostError,
}
