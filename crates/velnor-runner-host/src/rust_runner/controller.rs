// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;
use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::time::{Duration, Instant};

use tokio::task::{JoinError, JoinSet};
use velnor_runner_github::QueueSession;

use crate::config::HostConfig;
use crate::daemon_lock::DaemonLock;
use crate::error::{HostError, RustRunnerFailure};
use crate::launch_blocking::ListenFault;
use crate::listen::PollPathResult;
use crate::rust_runner::{ReservationId, RustRunnerLauncher, census};

mod admission;
mod batch;
mod cleanup;
mod connection;
mod cursor;
mod github;
mod jobs;
pub(in crate::rust_runner) mod owned;
mod pats;
mod publish;
mod readiness;
mod retire;
mod run_blocking;
mod runner;
mod sessions;
mod source;
mod startup;
mod stop;

use admission::{Admission, Closed, JobExit, Slots};
use cleanup::CleanupProof;
use connection::Live;
use github::Acquisition;
use readiness::Phase;
use runner::RunnerHealth;
use source::{LaunchGrant, Refusal};
use startup::Started;

pub use pats::{ConnectionPats, PatSource, load_connection_pat};
pub(crate) use readiness::assess as heartbeat_readiness;
pub(in crate::rust_runner) use readiness::{fresh, now_unix_ms};
pub(crate) use run_blocking::run_blocking;
pub use run_blocking::{Handover, ShutdownSignal, supervise_rust_runner_host};

const TICK: Duration = Duration::from_secs(2);
const POLL_WAKE: Duration = Duration::from_millis(500);
const MACHINE_PROBE: Duration = Duration::from_secs(10);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RustRunnerExit {
    Shutdown,
    ConfigChanged,
}

pub(super) struct Shutdown<'a>(Pin<Box<dyn Future<Output = ()> + 'a>>);

impl Shutdown<'_> {
    pub(super) async fn requested(&mut self) {
        (&mut self.0).await;
    }
}

pub(super) struct Context {
    state: PathBuf,
    config_sha256: String,
    launcher: RustRunnerLauncher,
}

impl Context {
    fn beat(&self, phase: Phase) {
        if let Err(error) = readiness::write(&self.state, &self.config_sha256, phase) {
            eprintln!("rust-runner heartbeat held: {error}");
        }
    }

    fn beat_idle(&self, phase: Phase) {
        self.beat(phase);
        if let Err(error) = census::publish_idle(&self.state, &self.config_sha256) {
            eprintln!("rust-runner census held: {error}");
        }
    }
}

enum Event {
    Polled {
        connection: usize,
        result: Result<PollPathResult, ListenFault>,
    },
    Refreshed {
        connection: usize,
        result: Result<(github::Admin, QueueSession), ListenFault>,
    },
    Renewed {
        connection: usize,
        result: Result<github::Admin, ListenFault>,
    },
    Checked {
        connection: usize,
        request_id: i64,
        result: Result<LaunchGrant, Refusal>,
    },
    Acquired {
        connection: usize,
        reservation: ReservationId,
        result: Result<Acquisition, ListenFault>,
    },
    Acked {
        connection: usize,
        message_id: i64,
        result: Result<(), ListenFault>,
    },
    Ended {
        reservation: ReservationId,
        exit: JobExit,
        binary: String,
    },
    Proved {
        reservation: ReservationId,
        result: Result<CleanupProof, ListenFault>,
    },
    Closed {
        connection: usize,
        result: Result<(), ListenFault>,
    },
    Machine(Result<(), ListenFault>),
    RunnerBinary(Result<String, ListenFault>),
}

pub(super) async fn blocking<T, F>(work: F) -> Result<T, ListenFault>
where
    T: Send + 'static,
    F: FnOnce() -> Result<T, ListenFault> + Send + 'static,
{
    tokio::task::spawn_blocking(work)
        .await
        .unwrap_or_else(|_| Err(HostError::RustRunner(RustRunnerFailure::TaskFailed).into()))
}

#[must_use]
pub fn host_toml_sha256(state: &Path) -> Option<String> {
    crate::readiness_scan::controller_config(state).map(|(_, sha256)| sha256)
}

struct Run<'a> {
    cx: Context,
    config: HostConfig,
    daemon_lock: &'a DaemonLock,
    connections: Vec<Live>,
    slots: Slots,
    taken: BTreeSet<(usize, i64)>,
    next_reservation: u64,
    tasks: JoinSet<Event>,
    machine_ok: bool,
    machine_checking: bool,
    machine_probed: Instant,
    runner: &'a mut RunnerHealth,
    stopping: bool,
}

pub async fn run_rust_runner_host(
    config: HostConfig,
    pats: ConnectionPats,
    state: &Path,
    daemon_lock: &DaemonLock,
    launcher: RustRunnerLauncher,
    shutdown: impl Future<Output = ()>,
) -> Result<RustRunnerExit, ListenFault> {
    let mut shutdown = Shutdown(Box::pin(shutdown));
    let mut health = RunnerHealth::new();
    run_with_health(
        config,
        pats,
        state,
        daemon_lock,
        launcher,
        &mut shutdown,
        &mut health,
    )
    .await
}

async fn run_with_health(
    config: HostConfig,
    pats: ConnectionPats,
    state: &Path,
    daemon_lock: &DaemonLock,
    launcher: RustRunnerLauncher,
    shutdown: &mut Shutdown<'_>,
    health: &mut RunnerHealth,
) -> Result<RustRunnerExit, ListenFault> {
    let state = std::fs::canonicalize(state).map_err(|_| HostError::Path)?;
    if !state.is_dir() || !daemon_lock.matches_path(&state.join("daemon.lock")) {
        return Err(HostError::Lock.into());
    }
    let (file_config, config_sha256) =
        crate::readiness_scan::controller_config(&state).ok_or(HostError::Config)?;
    if file_config != config {
        return Ok(RustRunnerExit::ConfigChanged);
    }
    let cx = Context {
        state,
        config_sha256,
        launcher,
    };
    let (connections, _hold) = match startup::start(&cx, &config, &pats, shutdown).await? {
        Started::Ready(connections, hold) => (connections, hold),
        Started::Shutdown => return Ok(RustRunnerExit::Shutdown),
        Started::ConfigChanged => return Ok(RustRunnerExit::ConfigChanged),
    };
    let capacity = config.host.max_jobs;
    let mut run = Run {
        cx,
        config,
        daemon_lock,
        connections,
        slots: Slots::new(capacity),
        taken: BTreeSet::new(),
        next_reservation: 1,
        tasks: JoinSet::new(),
        machine_ok: true,
        machine_checking: false,
        machine_probed: Instant::now(),
        runner: health,
        stopping: false,
    };
    let result = run.serve(shutdown).await;
    if let Err(fault) = &result
        && !matches!(fault, ListenFault::Host(HostError::Lock))
        && !run.stopping
        && let Err(error) = run.stop().await
    {
        eprintln!("rust-runner cleanup after a controller error held: {error}");
    }
    result
}

impl Run<'_> {
    async fn serve(&mut self, shutdown: &mut Shutdown<'_>) -> Result<RustRunnerExit, ListenFault> {
        let mut tick = tokio::time::interval(TICK);
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        let mut polls = tokio::time::interval(POLL_WAKE);
        polls.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            self.spawn_polls();
            tokio::select! {
                biased;
                () = shutdown.requested() => {
                    self.stop().await?;
                    return Ok(RustRunnerExit::Shutdown);
                }
                Some(joined) = self.tasks.join_next() => self.handle(joined)?,
                _ = polls.tick() => {}
                _ = tick.tick() => {
                    self.retire_removed();
                    self.on_tick()?;
                    self.close_retired();
                    if self.restart_due() && self.slots.is_empty() && self.idle() {
                        self.close_sessions().await;
                        return Ok(RustRunnerExit::ConfigChanged);
                    }
                }
            }
        }
    }

    fn idle(&self) -> bool {
        self.connections.iter().all(|live| live.batch.is_none())
    }

    fn handle(&mut self, joined: Result<Event, JoinError>) -> Result<(), ListenFault> {
        let event = joined.map_err(|_| HostError::RustRunner(RustRunnerFailure::TaskFailed))?;
        match event {
            Event::Polled { connection, result } => self.on_polled(connection, result),
            Event::Refreshed { connection, result } => self.on_refreshed(connection, result)?,
            Event::Renewed { connection, result } => self.on_renewed(connection, result),
            Event::Checked {
                connection,
                request_id,
                result,
            } => self.on_checked(connection, request_id, result),
            Event::Acquired {
                connection,
                reservation,
                result,
            } => self.on_acquired(connection, reservation, result),
            Event::Acked {
                connection,
                message_id,
                result,
            } => self.on_acked(connection, message_id, result),
            Event::Ended {
                reservation,
                exit,
                binary,
            } => {
                self.note_exit(&exit, &binary);
                self.begin_proof(reservation, exit);
            }
            Event::Proved {
                reservation,
                result,
            } => self.on_proved(reservation, result),
            Event::RunnerBinary(result) => {
                if self.runner.checked(result.ok()) {
                    eprintln!("rust-runner runner binary changed; admission reopens");
                }
            }
            Event::Closed { connection, result } => self.on_closed(connection, result),
            Event::Machine(result) => {
                self.machine_checking = false;
                self.machine_probed = Instant::now();
                match result {
                    Ok(()) => self.machine_ok = true,
                    Err(error) => {
                        eprintln!("rust-runner machine check held: {error}");
                        self.machine_ok = false;
                    }
                }
            }
        }
        Ok(())
    }

    fn on_tick(&mut self) -> Result<(), ListenFault> {
        if !self
            .daemon_lock
            .matches_path(&self.cx.state.join("daemon.lock"))
        {
            return Err(HostError::Lock.into());
        }
        self.cx.beat(if !self.machine_ok {
            Phase::MachineUnavailable
        } else if self.runner.is_failing() {
            Phase::RunnerFailing
        } else if self.connections.iter().any(Live::session_failing) {
            Phase::SessionFailing
        } else {
            Phase::Ready
        });
        self.publish_census();
        self.spawn_due_proofs();
        self.spawn_due_renewals();
        self.check_machine();
        self.check_runner();
        Ok(())
    }

    fn config_changed(&self) -> bool {
        host_toml_sha256(&self.cx.state).as_deref() != Some(self.cx.config_sha256.as_str())
    }

    fn admission(&self) -> Admission {
        if self.stopping {
            Admission::Closed(Closed::Stopping)
        } else if !self.machine_ok {
            Admission::Closed(Closed::MachineUnavailable)
        } else if self.runner.is_failing() {
            Admission::Closed(Closed::RunnerFailing)
        } else if !census::marker_absent(&self.cx.state.join("drain")) {
            Admission::Closed(Closed::Draining)
        } else if self.config_changed() {
            Admission::Closed(Closed::ConfigChanged)
        } else {
            Admission::Open {
                free: self.slots.free(),
            }
        }
    }
}
