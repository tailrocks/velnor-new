// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::path::Path;
use std::time::Duration;

use tokio::signal::unix::{Signal, SignalKind, signal};

use super::pats::ConnectionPats;
use super::readiness::Phase;
use super::startup::guarded;
use super::{Context, RustRunnerExit, Shutdown, host_toml_sha256, run_rust_runner_host};
use crate::config::{HostConfig, LaunchMode};
use crate::daemon_lock::DaemonLock;
use crate::error::HostError;
use crate::host_controller::config_watch::read_host_config;
use crate::launch_blocking::ListenFault;
use crate::rust_runner::RustRunnerLauncher;

const CREDENTIAL_RETRY: Duration = Duration::from_secs(30);
const CREDENTIAL_BUDGET: Duration = Duration::from_secs(30);
const CONFIG_RETRY: Duration = Duration::from_secs(5);

pub(crate) fn run_blocking(
    config: &HostConfig,
    state: &Path,
    daemon_lock: &DaemonLock,
) -> Result<(), ListenFault> {
    let _controller_run = daemon_lock.begin_controller_run()?;
    let state = std::fs::canonicalize(state).map_err(|_| HostError::Path)?;
    if !state.is_dir() || !daemon_lock.matches_path(&state.join("daemon.lock")) {
        return Err(HostError::Lock.into());
    }
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|_| HostError::Journal)?;
    let result = runtime.block_on(supervise(config.clone(), &state, daemon_lock));
    runtime.shutdown_background();
    result
}

struct Signals {
    terminate: Signal,
    interrupt: Signal,
}

impl Signals {
    fn install() -> Result<Self, HostError> {
        Ok(Self {
            terminate: signal(SignalKind::terminate()).map_err(|_| HostError::Journal)?,
            interrupt: signal(SignalKind::interrupt()).map_err(|_| HostError::Journal)?,
        })
    }

    async fn received(&mut self) {
        tokio::select! {
            _ = self.terminate.recv() => {}
            _ = self.interrupt.recv() => {}
        }
    }
}

async fn supervise(
    mut config: HostConfig,
    state: &Path,
    daemon_lock: &DaemonLock,
) -> Result<(), ListenFault> {
    let mut signals = Signals::install()?;
    loop {
        let sha = host_toml_sha256(state).ok_or(HostError::Config)?;
        let cx = Context {
            state: state.to_path_buf(),
            config_sha256: sha,
            launcher: RustRunnerLauncher::from_environment(),
        };
        let mut shutdown = Shutdown(Box::pin(signals.received()));
        let Some(pats) = load_pats(&cx, &config, &mut shutdown).await else {
            return Ok(());
        };
        let exit = run_rust_runner_host(
            config.clone(),
            pats,
            state,
            daemon_lock,
            cx.launcher.clone(),
            async move { shutdown.requested().await },
        )
        .await?;
        match exit {
            RustRunnerExit::Shutdown => return Ok(()),
            RustRunnerExit::ConfigChanged => {
                let Some(next) = reread(state, &mut signals).await else {
                    return Ok(());
                };
                if next.host.launch_mode != LaunchMode::RustRunner {
                    return Ok(());
                }
                config = next;
            }
        }
    }
}

async fn load_pats(
    cx: &Context,
    config: &HostConfig,
    shutdown: &mut Shutdown<'_>,
) -> Option<ConnectionPats> {
    loop {
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
            Some(Ok(pats)) => return Some(pats),
            Some(Err(error)) => eprintln!("rust-runner credentials unavailable: {error}"),
            None => return None,
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
            return None;
        }
    }
}

async fn reread(state: &Path, signals: &mut Signals) -> Option<HostConfig> {
    loop {
        match read_host_config(&state.join("host.toml")) {
            Ok(Some(config)) => return Some(config),
            Ok(None) | Err(_) => eprintln!("rust-runner config reload held"),
        }
        tokio::select! {
            () = signals.received() => return None,
            () = tokio::time::sleep(CONFIG_RETRY) => {}
        }
    }
}
