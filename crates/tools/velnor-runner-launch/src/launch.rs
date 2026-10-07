//! Acquire jobs up to capacity, mint JIT, and start one paired worker each.
//!
//! The journal row is committed before acquire, JIT, and Docker. JIT bytes are
//! passed to the starter and are not written to the journal.

use std::fmt;
use std::future::Future;

use zeroize::Zeroize;

use velnor_runner_github::{
    Poll, QueueSession, SessionError, SessionRequest, Transport, WireError,
};

use velnor_runner_host::HostError;
use velnor_runner_host::ensure_product_scale_set;
use velnor_runner_host::listen::{Link, Secret, admin_link};
use velnor_runner_host::scale_set::EnsureError;
use velnor_runner_host::worker::Started;
use velnor_runner_journal::journal::Journal;
use velnor_runner_journal::reconcile::Reconcile;
use velnor_runner_launch_slot as slot;

mod bind;
mod capacity;
mod docker_stub;
mod fakes;
mod gate;
pub mod harness;
mod host_lane;
mod inspect;

mod mint_origin;
mod session;
mod steps;
mod trace;
mod turn;

pub(crate) use capacity::{install_job_capacity, job_capacity};

pub use capacity::Admit;

use host_lane::HostLane;

#[cfg(test)]
pub(crate) use capacity::{
    Seat, admit, needs_running, parse_admit_target, poll_limit, statistics_blocked, wide_poll_limit,
};
#[cfg(test)]
pub(crate) use steps::{Idle, idle};
pub use turn::admission;

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
    let ceiling = job_capacity();
    let capacity = velnor_runner_host::guest::discover_guest_capacity(docker, ceiling)
        .await
        .map_err(|_| EnsureError::Unexpected {
            status: 0,
            step: "docker budget",
        })?;
    let _capacity = install_job_capacity(capacity);
    slot::release_exited(journal, docker).await?;
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
    let (mut session, row) =
        session::open_session(&mut link, set.id, admin.expose(), journal).await?;
    let driven = turn::poll_and_drive(
        &mut link,
        set.id,
        &mut session,
        admin.expose(),
        journal,
        docker,
    )
    .await;
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

    /// Current path after selecting the message queue origin.
    fn message_queue_path(&self, fallback: &str) -> String {
        fallback.to_owned()
    }

    /// Refresh the same session after one queue 401 and prepare the single replay.
    ///
    /// `ack_suffix` is present only for a message DELETE. Acquire replay stays on
    /// the admin origin and keeps its original request path and body.
    fn refresh_queue(
        &mut self,
        request: &mut SessionRequest,
        ack_suffix: Option<&str>,
    ) -> Result<(), SessionError> {
        let _ = (request, ack_suffix);
        Err(SessionError::Wire(WireError::Malformed))
    }
}

/// Acquire, JIT, and start for one poll. No session create.
///
/// # Errors
///
/// Returns [`EnsureError`] when more than one job is offered, or a later step fails.
/// An uncertain acquire is not acknowledged.
#[cfg(test)]
pub(crate) async fn drive_offer<T, S, F>(
    lane: &mut T,
    ctx: &Drive,
    polled: &Poll,
    journal: &Journal,
    start: S,
) -> Result<Option<Started>, EnsureError>
where
    T: velnor_runner_github::Transport + Lane,
    S: FnOnce(&str, &[u8], bind::Bind) -> F,
    F: Future<Output = Result<Started, HostError>>,
{
    drive_offer_tracked(lane, ctx, polled, journal, start)
        .await
        .map(|outcome| outcome.started)
}

/// Worker start plus the queue message deleted by this exact successful offer.
#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct DriveOutcome {
    /// Worker newly started by the offer, if any.
    pub(crate) started: Option<Started>,
    /// Message whose DELETE returned success after the worker path completed.
    pub(crate) acknowledged_message_id: Option<i64>,
}

/// Same start path as the test-only convenience wrapper, retaining positive ACK evidence.
pub(crate) async fn drive_offer_tracked<T, S, F>(
    lane: &mut T,
    ctx: &Drive,
    polled: &Poll,
    journal: &Journal,
    start: S,
) -> Result<DriveOutcome, EnsureError>
where
    T: velnor_runner_github::Transport + Lane,
    S: FnOnce(&str, &[u8], bind::Bind) -> F,
    F: Future<Output = Result<Started, HostError>>,
{
    if matches!(steps::idle(polled), steps::Idle::Scale) {
        let Poll::Batch(batch) = polled else {
            return Ok(DriveOutcome::default());
        };
        let started = steps::scale_id(lane, ctx, batch, journal, start).await?;
        return Ok(DriveOutcome {
            started,
            acknowledged_message_id: Some(batch.message_id),
        });
    }
    let Some((batch, request_id)) = steps::assignment(polled)? else {
        return Ok(DriveOutcome::default());
    };
    let started = steps::launch_id(lane, ctx, batch, journal, request_id, start).await?;
    Ok(DriveOutcome {
        started,
        acknowledged_message_id: Some(batch.message_id),
    })
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
        queue_path: String::new(),
        session: None,
        set_id,
        admin_token,
    };
    let name = runner_name(&session.session_id);
    steps::scale_unacked(&mut lane, &ctx, journal, &name, |volume, jit, bind| {
        let volume = volume.to_owned();
        let payload = jit.to_vec();
        async move { bind::start_bound(docker, &volume, &payload, &bind).await }
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
    queue_token: String,
    admin_token: &'a str,
    path: String,
    polled: &'a Poll,
}

async fn drive_ready<T>(
    lane: &mut T,
    ready: Ready<'_>,
    journal: &Journal,
    docker: &bollard::Docker,
    capacity: u32,
) -> Result<DriveOutcome, EnsureError>
where
    T: Transport + Lane,
{
    if slot::busy(journal, docker, capacity).await? {
        return Ok(DriveOutcome::default());
    }
    let ctx = Drive {
        set_id: ready.set_id,
        queue_path: ready.path,
        queue_token: ready.queue_token,
        admin_token: ready.admin_token.to_owned(),
    };
    drive_offer_tracked(lane, &ctx, ready.polled, journal, |volume, jit, bind| {
        let volume = volume.to_owned();
        let payload = jit.to_vec();
        async move { bind::start_bound(docker, &volume, &payload, &bind).await }
    })
    .await
}

fn ack_ready(
    link: &mut Link,
    set_id: i64,
    session: &mut QueueSession,
    admin_token: &str,
    path: String,
    queue: Option<String>,
    polled: &Poll,
) -> Result<Option<i64>, EnsureError> {
    let Poll::Batch(batch) = polled else {
        return Ok(None);
    };
    let ctx = Drive {
        set_id,
        queue_path: path,
        queue_token: session.token().to_owned(),
        admin_token: admin_token.to_owned(),
    };
    let admin = link.base().to_owned();
    let mut lane = HostLane {
        link,
        admin,
        queue,
        queue_path: String::new(),
        session: Some(session),
        set_id,
        admin_token,
    };
    steps::acknowledge(&mut lane, &ctx, batch)?;
    Ok(Some(batch.message_id))
}

#[cfg(test)]
mod tests;
