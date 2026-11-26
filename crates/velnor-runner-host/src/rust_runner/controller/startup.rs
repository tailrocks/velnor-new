// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::future::Future;
use std::time::Duration;

use velnor_runner_github::QueueSession;
use zeroize::Zeroizing;

use super::connection::{Live, Pending};
use super::github::{self, Admin};
use super::pats::ConnectionPats;
use super::readiness::Phase;
use super::{Context, Shutdown, TICK, blocking};
use crate::config::HostConfig;
use crate::error::HostError;
use crate::launch_blocking::ListenFault;
use crate::rust_runner::RustRunnerLauncher;

const STARTUP_RETRY: Duration = Duration::from_secs(30);

pub(super) enum Started {
    Ready(Vec<Live>),
    Shutdown,
}

#[derive(Clone, Copy)]
enum Step {
    Machine,
    Sweep,
    Set(usize),
    Session(usize),
    Clear(usize),
}

enum Done {
    Machine,
    Swept,
    Set(usize, Admin, i64),
    Session(usize, QueueSession),
    Cleared(usize),
}

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
            _ = tick.tick() => cx.beat(phase),
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
    let (mut machine, mut swept) = (false, false);
    while let Some(step) = next_step(&pending, machine, swept) {
        let work = prepare(step, &cx.launcher, &pending)?;
        let finish = matches!(step, Step::Session(_));
        let outcome = guarded(cx, shutdown, Phase::Startup, blocking(work), finish).await;
        let mut held = false;
        match outcome.value {
            Some(Ok(done)) => match done {
                Done::Machine => machine = true,
                Done::Swept => swept = true,
                Done::Set(index, admin, id) => pending[index].set = Some((admin, id)),
                Done::Session(index, session) => pending[index].session = Some(session),
                Done::Cleared(index) => pending[index].cleared = true,
            },
            Some(Err(error)) => {
                eprintln!("rust-runner startup held: {error}");
                held = true;
            }
            None => {}
        }
        if outcome.stopped {
            close_pending(&pending).await;
            return Ok(Started::Shutdown);
        }
        if held {
            let wait = guarded(
                cx,
                shutdown,
                Phase::Startup,
                tokio::time::sleep(STARTUP_RETRY),
                false,
            )
            .await;
            if wait.stopped {
                close_pending(&pending).await;
                return Ok(Started::Shutdown);
            }
        }
    }
    let mut live = Vec::with_capacity(pending.len());
    for connection in pending {
        live.push(connection.into_live()?);
    }
    Ok(Started::Ready(live))
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
            session: None,
            cleared: false,
        });
    }
    Ok(pending)
}

fn next_step(pending: &[Pending], machine: bool, swept: bool) -> Option<Step> {
    if !machine {
        return Some(Step::Machine);
    }
    if !swept {
        return Some(Step::Sweep);
    }
    pending.iter().enumerate().find_map(|(index, connection)| {
        if connection.set.is_none() {
            Some(Step::Set(index))
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
    pending: &[Pending],
) -> Result<Work, ListenFault> {
    let launcher = launcher.clone();
    Ok(match step {
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
                github::open_scale_set(&pat, &binding)
                    .map(|(admin, id)| Done::Set(index, admin, id))
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

pub(super) async fn close_pending(pending: &[Pending]) {
    for connection in pending {
        let (Some((admin, id)), Some(session)) = (&connection.set, &connection.session) else {
            continue;
        };
        let (admin, id, session_id) = (admin.clone(), *id, session.session_id.clone());
        if let Err(error) = blocking(move || github::close_session(&admin, id, &session_id)).await {
            eprintln!("rust-runner session close held: {error}");
        }
    }
}
