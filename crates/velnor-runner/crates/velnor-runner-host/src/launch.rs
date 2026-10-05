//! Acquire jobs up to capacity, mint JIT, and start one paired worker each.
//!
//! The journal row is committed before acquire, JIT, and Docker. JIT bytes are
//! passed to the starter and are not written to the journal.

use std::fmt;
use std::future::Future;

use zeroize::Zeroize;

use velnor_runner_github::{
    Exchange, Poll, QueueSession, SessionRequest, Transport, TransportFail,
};

use crate::ensure_product_scale_set;
use crate::error::HostError;
use crate::journal::Journal;
use crate::listen::{Link, Secret, admin_link};
use crate::reconcile::Reconcile;
use crate::scale_set::EnsureError;
use crate::worker::{Started, start_pair};

mod capacity;
mod gate;
#[cfg(all(test, unix))]
mod initial_scale_tests;
mod inspect;
#[cfg(all(test, unix))]
mod inspect_tests;
mod session;
mod slot;
mod steps;
#[cfg(test)]
mod subject_tests;
mod trace;
mod turn;

pub(crate) use capacity::{install_job_capacity, job_capacity};

#[cfg(test)]
pub(crate) use capacity::{
    Admit, Seat, admit, needs_running, parse_admit_target, parse_job_capacity, poll_limit,
    wide_poll_limit,
};
#[cfg(test)]
pub(crate) use slot::occupies;
#[cfg(test)]
pub(crate) use steps::{Idle, idle};

/// What one launch attempt started. No JIT and no token.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LaunchReport {
    /// Scale-set id.
    pub set_id: i64,
    /// First worker started in this call. Absent when the queue had no job.
    pub started: Option<Started>,
    /// Every worker started in this call, in start order. No JIT and no token.
    pub workers: Vec<Started>,
}

/// Open one session, start the admitted workers, then delete that session.
///
/// # Errors
///
/// Returns [`EnsureError`] when registration, acquire, JIT, Docker, or the
/// session delete fails. A failed delete is returned even when the poll failed,
/// because a leaked session blocks the next create. The next open deletes only
/// session ids this journal recorded. Timeout after acquire keeps the journal
/// row uncertain and does not acknowledge. `VELNOR_RECONCILE=1` can fail the
/// journal read before a session exists, and a hold skips it. The session stays
/// open while an owned launch container is still running.
pub async fn launch_once(
    pat: &str,
    owner: &str,
    repo: &str,
    docker: &bollard::Docker,
    journal: &Journal,
) -> Result<LaunchReport, EnsureError> {
    let set = ensure_product_scale_set(pat, owner, repo)?;
    if std::env::var("VELNOR_RECONCILE").ok().as_deref() == Some("1") {
        let decision = gate::reconcile_gate(journal, docker).await?;
        eprintln!("reconcile={}", gate::gate_line(&decision));
        if let Reconcile::Hold { .. } = decision {
            return Ok(LaunchReport {
                set_id: set.id,
                started: None,
                workers: Vec::new(),
            });
        }
    }
    let mut link = admin_link(pat, owner, repo)?;
    let admin = Secret::new(link.token());
    let (session, row) = session::open_session(&mut link, set.id, admin.expose(), journal).await?;
    let driven =
        turn::poll_and_drive(&mut link, set.id, &session, admin.expose(), journal, docker).await;
    let closed = session::close_session(
        &mut link,
        set.id,
        &session.session_id,
        row,
        admin.expose(),
        journal,
    )
    .await;
    report(set.id, driven, closed)
}

/// Call context. Tokens are redacted in [`Debug`] and zeroized on drop.
pub(crate) struct Drive {
    /// Scale-set id.
    pub(crate) set_id: i64,
    /// Queue path. No host.
    pub(crate) queue_path: String,
    /// Queue bearer.
    pub(crate) queue_token: String,
    /// Admin bearer.
    pub(crate) admin_token: String,
}

impl fmt::Debug for Drive {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Drive")
            .field("set_id", &self.set_id)
            .field("queue_path", &self.queue_path)
            .field("queue_token", &"[redacted]")
            .field("admin_token", &"[redacted]")
            .finish()
    }
}

impl Drop for Drive {
    fn drop(&mut self) {
        self.queue_token.zeroize();
        self.admin_token.zeroize();
    }
}

/// Switch the client between the admin origin and the message host.
pub(crate) trait Lane {
    /// Use the admin origin for acquire, JIT, and session delete.
    ///
    /// # Errors
    ///
    /// Returns [`EnsureError`] when the origin cannot be selected.
    fn on_admin(&mut self) -> Result<(), EnsureError>;

    /// Use the message-host origin for acknowledgement.
    ///
    /// # Errors
    ///
    /// Returns [`EnsureError`] when the origin cannot be selected.
    fn on_queue(&mut self) -> Result<(), EnsureError>;
}

/// Acquire, JIT, and start for one poll. No session create.
///
/// # Errors
///
/// Returns [`EnsureError`] when more than one job is offered, or a later step fails.
/// An uncertain acquire is not acknowledged.
pub(crate) async fn drive_offer<T, S, F>(
    lane: &mut T,
    ctx: &Drive,
    polled: &Poll,
    journal: &Journal,
    start: S,
) -> Result<Option<Started>, EnsureError>
where
    T: velnor_runner_github::Transport + Lane,
    S: FnOnce(&str, &[u8]) -> F,
    F: Future<Output = Result<Started, HostError>>,
{
    if matches!(steps::idle(polled), steps::Idle::Scale) {
        let Poll::Batch(batch) = polled else {
            return Ok(None);
        };
        return steps::scale_id(lane, ctx, batch, journal, start).await;
    }
    let Some((batch, request_id)) = steps::assignment(polled)? else {
        return Ok(None);
    };
    steps::launch_id(lane, ctx, batch, journal, request_id, start).await
}

fn report(
    set_id: i64,
    driven: Result<Vec<Started>, EnsureError>,
    closed: Result<(), EnsureError>,
) -> Result<LaunchReport, EnsureError> {
    match (driven, closed) {
        (Ok(workers), Ok(())) => {
            let started = workers.first().cloned();
            Ok(LaunchReport {
                set_id,
                started,
                workers,
            })
        }
        (_, Err(error)) | (Err(error), Ok(())) => Err(error),
    }
}

async fn scale_session(
    link: &mut Link,
    set_id: i64,
    session: &QueueSession,
    admin_token: &str,
    journal: &Journal,
    docker: &bollard::Docker,
) -> Result<Option<Started>, EnsureError> {
    let population = session
        .statistics()
        .map_or(0, velnor_runner_github::Statistics::assigned_population);
    if population <= 0 {
        return Ok(None);
    }
    let ctx = Drive {
        set_id,
        queue_path: String::new(),
        queue_token: session.token().to_owned(),
        admin_token: admin_token.to_owned(),
    };
    let admin = link.base().to_owned();
    let mut lane = HostLane {
        link,
        admin,
        queue: None,
    };
    let name = runner_name(&session.session_id);
    steps::scale_unacked(&mut lane, &ctx, journal, &name, |volume, jit| {
        let volume = volume.to_owned();
        let payload = jit.to_vec();
        async move { start_pair(docker, &volume, &payload).await }
    })
    .await
}

fn runner_name(session_id: &str) -> String {
    let mut name = String::from("s");
    for ch in session_id.chars().filter(char::is_ascii_alphanumeric) {
        name.push(ch);
        if name.len() == 13 {
            break;
        }
    }
    if name.len() == 1 {
        name.push('0');
    }
    name
}

struct Ready<'a> {
    set_id: i64,
    session: &'a QueueSession,
    admin_token: &'a str,
    path: String,
    queue: Option<String>,
    polled: &'a Poll,
}

async fn drive_ready(
    link: &mut Link,
    ready: Ready<'_>,
    journal: &Journal,
    docker: &bollard::Docker,
    capacity: u32,
) -> Result<Option<Started>, EnsureError> {
    if slot::busy(journal, docker, capacity).await? {
        return held(link, ready);
    }
    let ctx = Drive {
        set_id: ready.set_id,
        queue_path: ready.path,
        queue_token: ready.session.token().to_owned(),
        admin_token: ready.admin_token.to_owned(),
    };
    let admin = link.base().to_owned();
    let mut lane = HostLane {
        link,
        admin,
        queue: ready.queue,
    };
    drive_offer(&mut lane, &ctx, ready.polled, journal, |volume, jit| {
        let volume = volume.to_owned();
        let payload = jit.to_vec();
        async move { start_pair(docker, &volume, &payload).await }
    })
    .await
}

fn held(link: &mut Link, ready: Ready<'_>) -> Result<Option<Started>, EnsureError> {
    if !matches!(steps::idle(ready.polled), steps::Idle::Scale) {
        return Ok(None);
    }
    ack_ready(link, ready.session, ready.path, ready.queue, ready.polled)?;
    Ok(None)
}

fn ack_ready(
    link: &mut Link,
    session: &QueueSession,
    path: String,
    queue: Option<String>,
    polled: &Poll,
) -> Result<(), EnsureError> {
    let Poll::Batch(batch) = polled else {
        return Ok(());
    };
    let ctx = Drive {
        set_id: 0,
        queue_path: path,
        queue_token: session.token().to_owned(),
        admin_token: String::new(),
    };
    let admin = link.base().to_owned();
    let mut lane = HostLane { link, admin, queue };
    steps::acknowledge(&mut lane, &ctx, batch)
}

struct HostLane<'a> {
    link: &'a mut Link,
    admin: String,
    queue: Option<String>,
}

impl Transport for HostLane<'_> {
    fn exchange(&mut self, request: &SessionRequest) -> Result<Exchange, TransportFail> {
        self.link.transport().exchange(request)
    }
}

impl Lane for HostLane<'_> {
    fn on_admin(&mut self) -> Result<(), EnsureError> {
        let admin = self.admin.clone();
        self.link.set_base(&admin)
    }

    fn on_queue(&mut self) -> Result<(), EnsureError> {
        let Some(origin) = self.queue.clone() else {
            return Ok(());
        };
        self.link.set_base(&origin)
    }
}
