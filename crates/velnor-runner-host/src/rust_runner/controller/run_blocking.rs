// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::path::Path;
use std::time::Duration;

use tokio::runtime::Runtime;

use super::pats::ConnectionPats;
use super::readiness::Phase;
use super::runner::RunnerHealth;
use super::startup::guarded;
use super::{Context, RustRunnerExit, Shutdown, host_toml_sha256, run_with_health};
use crate::config::{HostConfig, LaunchMode};
use crate::daemon_lock::DaemonLock;
use crate::error::HostError;
use crate::launch_blocking::ListenFault;
use crate::readiness_scan::controller_config;
use crate::rust_runner::RustRunnerLauncher;
use crate::shutdown_signals::Signals;

const CREDENTIAL_RETRY: Duration = Duration::from_secs(30);
const CREDENTIAL_BUDGET: Duration = Duration::from_secs(30);
const CONFIG_RETRY: Duration = Duration::from_secs(5);

pub(crate) enum Handover {
    Stopped,
    OfficialRunner(Box<HostConfig>),
    RustRunner,
}

pub(crate) fn run_blocking(
    runtime: &Runtime,
    state: &Path,
    daemon_lock: &DaemonLock,
    signals: &mut Signals,
) -> Result<Handover, ListenFault> {
    let _controller_run = daemon_lock.begin_controller_run()?;
    let state = std::fs::canonicalize(state).map_err(|_| HostError::Path)?;
    if !state.is_dir() || !daemon_lock.matches_path(&state.join("daemon.lock")) {
        return Err(HostError::Lock.into());
    }
    runtime.block_on(supervise(&state, daemon_lock, signals))
}

async fn supervise(
    state: &Path,
    daemon_lock: &DaemonLock,
    signals: &mut Signals,
) -> Result<Handover, ListenFault> {
    let mut health = RunnerHealth::new();
    let mut snapshot = Some(controller_config(state).ok_or(HostError::Config)?);
    loop {
        let (config, config_sha256) = match snapshot.take() {
            Some(current) => current,
            None => match reread(state, signals).await {
                Some(next) => next,
                None => return Ok(Handover::Stopped),
            },
        };
        if config.host.launch_mode != LaunchMode::RustRunner {
            eprintln!(
                "rust-runner controller: launch mode changed; handing over to the official-runner controller"
            );
            return Ok(Handover::OfficialRunner(Box::new(config)));
        }
        let cx = Context {
            state: state.to_path_buf(),
            config_sha256,
            launcher: RustRunnerLauncher::from_environment(),
        };
        let mut shutdown = Shutdown(Box::pin(signals.received()));
        let pats = match load_pats(&cx, &config, &mut shutdown).await {
            Credentials::Loaded(pats) => pats,
            Credentials::ConfigChanged => continue,
            Credentials::Stopped => return Ok(Handover::Stopped),
        };
        let exit = run_with_health(
            config,
            pats,
            state,
            daemon_lock,
            cx.launcher.clone(),
            &mut shutdown,
            &mut health,
        )
        .await?;
        match exit {
            RustRunnerExit::Shutdown => return Ok(Handover::Stopped),
            RustRunnerExit::ConfigChanged => {}
        }
    }
}

enum Credentials {
    Loaded(ConnectionPats),
    ConfigChanged,
    Stopped,
}

async fn load_pats(cx: &Context, config: &HostConfig, shutdown: &mut Shutdown<'_>) -> Credentials {
    loop {
        if host_toml_sha256(&cx.state).as_deref() != Some(cx.config_sha256.as_str()) {
            return Credentials::ConfigChanged;
        }
        let deadline = tokio::time::Instant::now() + CREDENTIAL_BUDGET;
        let loaded = guarded(
            cx,
            shutdown,
            Phase::WaitingForCredentials,
            ConnectionPats::load(config, deadline),
            false,
        )
        .await;
        match loaded.value {
            Some(Ok(pats)) => return Credentials::Loaded(pats),
            Some(Err(error)) => eprintln!("rust-runner credentials unavailable: {error}"),
            None => return Credentials::Stopped,
        }
        let wait = guarded(
            cx,
            shutdown,
            Phase::WaitingForCredentials,
            tokio::time::sleep(CREDENTIAL_RETRY),
            false,
        )
        .await;
        if wait.stopped {
            return Credentials::Stopped;
        }
    }
}

async fn reread(state: &Path, signals: &mut Signals) -> Option<(HostConfig, String)> {
    loop {
        if let Some(current) = controller_config(state) {
            return Some(current);
        }
        eprintln!("rust-runner config reload held");
        tokio::select! {
            () = signals.received() => return None,
            () = tokio::time::sleep(CONFIG_RETRY) => {}
        }
    }
}
