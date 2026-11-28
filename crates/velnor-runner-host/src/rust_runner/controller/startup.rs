// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::future::Future;
use std::path::Path;
use std::time::{Duration, Instant};

use velnor_runner_github::QueueSession;
use zeroize::Zeroizing;

use super::connection::{ADMIN_RENEWAL, Live, Pending};
use super::github::{self, Admin};
use super::pats::ConnectionPats;
use super::readiness::Phase;
use super::sessions::{OpenSession, close_all};
use super::{Context, Shutdown, TICK, blocking, host_toml_sha256};
use crate::config::HostConfig;
use crate::error::HostError;
use crate::launch_blocking::ListenFault;
use crate::rust_runner::{HostHold, RustRunnerLauncher};

const STARTUP_RETRY: Duration = Duration::from_secs(30);

pub(super) enum Started {
    Ready(Vec<Live>, HostHold),
    Shutdown,
    ConfigChanged,
}

#[derive(Clone, Copy)]
enum Step {
    Hold,
    Machine,
    Sweep,
    Set(usize),
    Renew(usize),
    Session(usize),
    Clear(usize),
}

impl Step {
    const fn held_renewal(self) -> Option<usize> {
        match self {
            Self::Session(index) | Self::Clear(index) => Some(index),
            Self::Hold | Self::Machine | Self::Sweep | Self::Set(_) | Self::Renew(_) => None,
        }
    }
}

fn renew_held(step: Step, admin_age: Duration) -> Option<usize> {
    step.held_renewal().filter(|_| admin_age >= ADMIN_RENEWAL)
}

enum Done {
    Held(HostHold),
    Machine,
    Swept,
    Set(usize, Admin, i64),
    Renewed(usize, Admin),
    Session(usize, QueueSession),
    Cleared(usize),
}

#[cfg(test)]
mod tests;

pub(super) struct Guarded<T> {
    pub(super) value: Option<T>,
    pub(super) stopped: bool,
}

pub(super) async fn guarded<T>(
    cx: &Context,
    shutdown: &mut Shutdown<'_>,
    phase: Phase,
    work: impl Future<Output = T>,
    finish: bool,
) -> Guarded<T> {
    tokio::pin!(work);
    let mut tick = tokio::time::interval(TICK);
    let mut stopped = false;
    loop {
        tokio::select! {
            biased;
            () = shutdown.requested(), if !stopped => {
                if !finish {
                    return Guarded { value: None, stopped: true };
                }
                stopped = true;
            }
            output = &mut work => return Guarded { value: Some(output), stopped },
            _ = tick.tick() => cx.beat_idle(phase),
        }
    }
}

pub(super) async fn start(
    cx: &Context,
    config: &HostConfig,
    pats: &ConnectionPats,
    shutdown: &mut Shutdown<'_>,
) -> Result<Started, ListenFault> {
    let mut pending = pending_connections(config, pats)?;
    let (mut store_lock, mut machine, mut swept) = (None, false, false);
    while let Some(step) = next_step(&pending, store_lock.is_some(), machine, swept) {
        if config_changed(cx) {
            return Ok(close_until_closed(cx, shutdown, &pending).await);
        }
        let work = prepare(step, &cx.launcher, &cx.state, &pending)?;
        let finish = matches!(step, Step::Session(_));
        let outcome = guarded(cx, shutdown, Phase::Startup, blocking(work), finish).await;
        let mut held = false;
        match outcome.value {
            Some(Ok(done)) => match done {
                Done::Held(acquired) => store_lock = Some(acquired),
                Done::Machine => machine = true,
                Done::Swept => swept = true,
                Done::Set(index, admin, id) => {
                    pending[index].set = Some((admin, id));
                    pending[index].admin_at = Instant::now();
                }
                Done::Renewed(index, admin) => {
                    if let Some((current, _)) = pending[index].set.as_mut() {
                        *current = admin;
                    }
                    pending[index].admin_at = Instant::now();
                    pending[index].renew = false;
                }
                Done::Session(index, session) => pending[index].session = Some(session),
                Done::Cleared(index) => pending[index].cleared = true,
            },
            Some(Err(error)) => {
                eprintln!("rust-runner startup held: {error}");
                held = true;
                let age = step
                    .held_renewal()
                    .and_then(|index| pending.get(index))
                    .map(|connection| connection.admin_age(Instant::now()));
                if let Some(index) = age.and_then(|age| renew_held(step, age)) {
                    pending[index].renew = true;
                }
            }
            None => {}
        }
        if outcome.stopped {
            close_pending(&pending).await;
            return Ok(Started::Shutdown);
        }
        if held {
            let retry_at = Instant::now() + STARTUP_RETRY;
            while Instant::now() < retry_at {
                let wait = guarded(
                    cx,
                    shutdown,
                    Phase::Startup,
                    tokio::time::sleep(TICK),
                    false,
                )
                .await;
                if wait.stopped {
                    close_pending(&pending).await;
                    return Ok(Started::Shutdown);
                }
                if config_changed(cx) {
                    return Ok(close_until_closed(cx, shutdown, &pending).await);
                }
            }
        }
    }
    let mut live = Vec::with_capacity(pending.len());
    for connection in pending {
        live.push(connection.into_live()?);
    }
    let store_lock = store_lock.ok_or(HostError::Config)?;
    Ok(Started::Ready(live, store_lock))
}

fn config_changed(cx: &Context) -> bool {
    host_toml_sha256(&cx.state).as_deref() != Some(cx.config_sha256.as_str())
}

fn pending_connections(
    config: &HostConfig,
    pats: &ConnectionPats,
) -> Result<Vec<Pending>, ListenFault> {
    let mut pending = Vec::with_capacity(config.connections().len());
    for binding in config.connections() {
        let pat = pats
            .get(&binding.connection_id)
            .ok_or(HostError::Config)?
            .to_owned();
        pending.push(Pending {
            binding: binding.clone(),
            pat: Zeroizing::new(pat),
            policy: config
                .validated_job_authority_policy(&binding.connection_id)?
                .ok_or(HostError::Config)?
                .clone(),
            set: None,
            admin_at: Instant::now(),
            renew: false,
            session: None,
            cleared: false,
        });
    }
    Ok(pending)
}

fn next_step(pending: &[Pending], held: bool, machine: bool, swept: bool) -> Option<Step> {
    if !held {
        return Some(Step::Hold);
    }
    if !machine {
        return Some(Step::Machine);
    }
    if !swept {
        return Some(Step::Sweep);
    }
    pending.iter().enumerate().find_map(|(index, connection)| {
        if connection.set.is_none() {
            Some(Step::Set(index))
        } else if connection.renew {
            Some(Step::Renew(index))
        } else if connection.session.is_none() {
            Some(Step::Session(index))
        } else if !connection.cleared {
            Some(Step::Clear(index))
        } else {
            None
        }
    })
}

type Work = Box<dyn FnOnce() -> Result<Done, ListenFault> + Send>;

fn prepare(
    step: Step,
    launcher: &RustRunnerLauncher,
    state: &Path,
    pending: &[Pending],
) -> Result<Work, ListenFault> {
    let launcher = launcher.clone();
    let state = state.to_path_buf();
    Ok(match step {
        Step::Hold => Box::new(move || launcher.hold_host().map(Done::Held).map_err(Into::into)),
        Step::Machine => Box::new(move || {
            launcher
                .ensure_machine()
                .map(|()| Done::Machine)
                .map_err(Into::into)
        }),
        Step::Sweep => Box::new(move || launcher.sweep().map(|()| Done::Swept).map_err(Into::into)),
        Step::Set(index) => {
            let connection = pending.get(index).ok_or(HostError::Config)?;
            let (pat, binding) = (connection.pat.clone(), connection.binding.clone());
            Box::new(move || {
                github::open_scale_set(&pat, &binding, &state)
                    .map(|(admin, id)| Done::Set(index, admin, id))
            })
        }
        Step::Renew(index) => {
            let connection = pending.get(index).ok_or(HostError::Config)?;
            let (pat, binding) = (connection.pat.clone(), connection.binding.clone());
            Box::new(move || {
                github::renew_admin(&pat, &binding).map(|admin| Done::Renewed(index, admin))
            })
        }
        Step::Session(index) => {
            let connection = pending.get(index).ok_or(HostError::Config)?;
            let (admin, id) = connection.set.clone().ok_or(HostError::Config)?;
            Box::new(move || {
                github::open_session(&admin, id).map(|session| Done::Session(index, session))
            })
        }
        Step::Clear(index) => {
            let connection = pending.get(index).ok_or(HostError::Config)?;
            let (admin, id) = connection.set.clone().ok_or(HostError::Config)?;
            Box::new(move || github::clear_runners(&admin, id).map(|()| Done::Cleared(index)))
        }
    })
}

fn open_sessions(pending: &[Pending]) -> Vec<OpenSession> {
    pending
        .iter()
        .filter_map(|connection| {
            let ((admin, id), session) = (connection.set.as_ref()?, connection.session.as_ref()?);
            Some((admin.clone(), *id, session.session_id.clone()))
        })
        .collect()
}

pub(super) async fn close_pending(pending: &[Pending]) {
    close_all(open_sessions(pending)).await;
}

async fn close_until_closed(
    cx: &Context,
    shutdown: &mut Shutdown<'_>,
    pending: &[Pending],
) -> Started {
    let mut remaining = open_sessions(pending);
    let mut backoff = TICK;
    loop {
        remaining = close_all(remaining).await;
        if remaining.is_empty() {
            return Started::ConfigChanged;
        }
        let retry_at = Instant::now() + backoff;
        backoff = (backoff * 2).min(STARTUP_RETRY);
        while Instant::now() < retry_at {
            let wait = guarded(
                cx,
                shutdown,
                Phase::Startup,
                tokio::time::sleep(TICK),
                false,
            )
            .await;
            if wait.stopped {
                return Started::Shutdown;
            }
        }
    }
}
