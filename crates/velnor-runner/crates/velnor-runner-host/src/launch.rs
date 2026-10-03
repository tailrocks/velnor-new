//! Acquire one job, mint JIT, and start the paired worker.
//!
//! The journal row is committed before acquire, JIT, and Docker. JIT bytes are
//! passed to the starter and are not written to the journal.

use std::fmt;
use std::future::Future;

use zeroize::Zeroize;

use velnor_runner_github::{
    Exchange, Poll, QueueSession, SessionRequest, Transport, TransportFail, create_session,
    delete_session,
};

use crate::ensure_product_scale_set;
use crate::error::HostError;
use crate::journal::Journal;
use crate::listen::{
    Link, OWNER_NAME, Secret, admin_link, annotate, map_listen, point_at_queue, poll_path,
    restore_base,
};
use crate::scale_set::EnsureError;
use crate::worker::{Started, start_pair};

mod slot;
mod steps;
mod trace;

#[cfg(test)]
pub(crate) use steps::{Idle, idle};

/// What one launch attempt started. No JIT and no token.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LaunchReport {
    /// Scale-set id.
    pub set_id: i64,
    /// Worker ids when a pair is running. Absent when the queue had no job.
    pub started: Option<Started>,
}

/// Open one session, start at most one worker, then delete that session.
///
/// # Errors
///
/// Returns [`EnsureError`] when registration, acquire, JIT, Docker, or the
/// session delete fails. A failed delete is returned even when the poll failed,
/// because a leaked session blocks the next create. Timeout after acquire keeps
/// the journal row uncertain and does not acknowledge.
pub async fn launch_once(
    pat: &str,
    owner: &str,
    repo: &str,
    docker: &bollard::Docker,
    journal: &Journal,
) -> Result<LaunchReport, EnsureError> {
    let set = ensure_product_scale_set(pat, owner, repo)?;
    let mut link = admin_link(pat, owner, repo)?;
    let admin = Secret::new(link.token());
    let session = create_session(link.transport(), set.id, OWNER_NAME, admin.expose())
        .map_err(|err| annotate(err, "create-session"))?;
    let driven = poll_and_drive(&mut link, set.id, &session, admin.expose(), journal, docker).await;
    let closed = delete_session(
        link.transport(),
        set.id,
        &session.session_id,
        admin.expose(),
    )
    .map_err(map_listen);
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
    driven: Result<Option<Started>, EnsureError>,
    closed: Result<(), EnsureError>,
) -> Result<LaunchReport, EnsureError> {
    match (driven, closed) {
        (Ok(started), Ok(())) => Ok(LaunchReport { set_id, started }),
        (_, Err(error)) | (Err(error), Ok(())) => Err(error),
    }
}

async fn poll_and_drive(
    link: &mut Link,
    set_id: i64,
    session: &QueueSession,
    admin_token: &str,
    journal: &Journal,
    docker: &bollard::Docker,
) -> Result<Option<Started>, EnsureError> {
    trace::session(session);
    let started = scale_session(link, set_id, session, admin_token, journal, docker).await?;
    let primed = started.is_some();
    for _ in 0..poll_bound() {
        let (saved, path) = point_at_queue(link, &session.message_queue_url)?;
        let queue = saved.as_ref().map(|_| link.base().to_owned());
        let polled = poll_path(link, session, &path);
        restore_base(link, saved)?;
        let polled = polled?;
        trace::batch(&polled);
        match steps::idle(&polled) {
            // A 202 is the long-poll timeout. A job can be queued while this
            // session is still open, so keep the session and poll again.
            steps::Idle::Empty => {}
            steps::Idle::Blocked => {
                return Err(EnsureError::Unexpected {
                    status: 0,
                    step: "queue",
                });
            }
            steps::Idle::Ack => ack_ready(link, session, path, queue, &polled)?,
            steps::Idle::Scale if started.is_some() => {
                ack_ready(link, session, path, queue, &polled)?;
                return Ok(started);
            }
            steps::Idle::Launch if started.is_some() => return Ok(started),
            steps::Idle::Launch | steps::Idle::Scale => {
                let launched = drive_ready(
                    link,
                    Ready {
                        set_id,
                        session,
                        admin_token,
                        path,
                        queue,
                        polled: &polled,
                    },
                    journal,
                    docker,
                )
                .await?;
                return Ok(started.or(launched));
            }
        }
        if primed {
            return Ok(started);
        }
    }
    Ok(started)
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

fn poll_bound() -> usize {
    let Ok(text) = std::env::var("VELNOR_LAUNCH_POLLS") else {
        return 8;
    };
    let Ok(bound) = text.parse::<usize>() else {
        return 8;
    };
    bound.clamp(1, 8)
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
) -> Result<Option<Started>, EnsureError> {
    if slot::busy(journal, docker).await? {
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
