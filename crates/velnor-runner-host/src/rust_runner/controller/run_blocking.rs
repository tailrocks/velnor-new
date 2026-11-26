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
use crate::launch_blocking::ListenFault;
use crate::readiness_scan::controller_config;
use crate::rust_runner::RustRunnerLauncher;

const CREDENTIAL_RETRY: Duration = Duration::from_secs(30);
const CREDENTIAL_BUDGET: Duration = Duration::from_secs(30);
const CONFIG_RETRY: Duration = Duration::from_secs(5);

pub(crate) fn run_blocking(state: &Path, daemon_lock: &DaemonLock) -> Result<(), ListenFault> {
    let _controller_run = daemon_lock.begin_controller_run()?;
    let state = std::fs::canonicalize(state).map_err(|_| HostError::Path)?;
    if !state.is_dir() || !daemon_lock.matches_path(&state.join("daemon.lock")) {
        return Err(HostError::Lock.into());
    }
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|_| HostError::Journal)?;
    let result = runtime.block_on(supervise(&state, daemon_lock));
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

async fn supervise(state: &Path, daemon_lock: &DaemonLock) -> Result<(), ListenFault> {
    let mut signals = Signals::install()?;
    let mut snapshot = Some(controller_config(state).ok_or(HostError::Config)?);
    loop {
        let (config, config_sha256) = match snapshot.take() {
            Some(current) => current,
            None => match reread(state, &mut signals).await {
                Some(next) => next,
                None => return Ok(()),
            },
        };
        if config.host.launch_mode != LaunchMode::RustRunner {
            return Ok(());
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
            Credentials::Stopped => return Ok(()),
        };
        let exit = run_rust_runner_host(
            config,
            pats,
            state,
            daemon_lock,
            cx.launcher.clone(),
            async move { shutdown.requested().await },
        )
        .await?;
        match exit {
            RustRunnerExit::Shutdown => return Ok(()),
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
