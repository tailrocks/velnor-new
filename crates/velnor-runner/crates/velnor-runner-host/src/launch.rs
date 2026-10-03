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

mod steps;

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

/// Open one session, launch at most one acquired job, then delete that session.
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
    for _ in 0..8 {
        let (saved, path) = point_at_queue(link, &session.message_queue_url)?;
        let queue = saved.as_ref().map(|_| link.base().to_owned());
        let polled = poll_path(link, session, &path);
        restore_base(link, saved)?;
        let polled = polled?;
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
            steps::Idle::Launch => {
                return drive_ready(
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
                .await;
            }
        }
    }
    Ok(None)
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
