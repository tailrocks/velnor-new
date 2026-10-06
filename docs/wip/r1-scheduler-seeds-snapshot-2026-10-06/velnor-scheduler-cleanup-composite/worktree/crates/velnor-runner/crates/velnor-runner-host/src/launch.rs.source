//! Acquire jobs up to capacity, mint JIT, and start one paired worker each.
//!
//! The journal row is committed before acquire, JIT, and Docker. JIT bytes are
//! passed to the starter and are not written to the journal.

use std::fmt;

use zeroize::Zeroize;

use velnor_runner_github::{Poll, QueueSession};

use crate::config::HostConfig;
use crate::daemon_lock::EngineLineageGuard;
use crate::ensure_product_scale_set;
use crate::error::HostError;
use crate::journal::{Journal, LaunchIdentity, LaunchReservation};
use crate::listen::{Link, Secret, admin_link};
use crate::reconcile::Reconcile;
use crate::scale_set::EnsureError;
use crate::worker::{
    PreparedDind, ResourceBudget, Started, prepare_dind_until, start_runner_until,
};

mod capacity;
mod completion;
mod gate;
#[cfg(all(test, unix))]
mod initial_scale_tests;
mod inspect;
#[cfg(all(test, unix))]
mod inspect_tests;
mod lane;
use lane::{HostLane, ack_ready};
mod names;
#[cfg(all(test, unix))]
mod recovery_tests;
mod session;
mod slot;
mod steps;
mod steps_ack;
mod steps_acquire;
#[cfg(test)]
mod subject_tests;
mod trace;
mod turn;

use names::runner_name;

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
    host_config: &HostConfig,
) -> Result<LaunchReport, EnsureError> {
    let _capacity = install_job_capacity(host_config.host.max_jobs);
    let resource_budget = host_config
        .resource_budget()
        .map_err(|_| EnsureError::Unexpected {
            status: 0,
            step: "worker-resource-budget",
        })?;
    let info = docker.info().await.map_err(|_| EnsureError::Unexpected {
        status: 0,
        step: "docker-info",
    })?;
    resource_budget
        .validate_guest_cpu(info.ncpu)
        .map_err(|_| EnsureError::Unexpected {
            status: 0,
            step: "worker-resource-budget",
        })?;
    let engine = info.id.ok_or(EnsureError::Unexpected {
        status: 0,
        step: "docker-engine-id",
    })?;
    let lineage = EngineLineageGuard::acquire(&engine).map_err(|_| EnsureError::Unexpected {
        status: 0,
        step: "engine-lineage",
    })?;
    journal
        .establish_engine_lineage(&engine, lineage)
        .await
        .map_err(|_| EnsureError::Unexpected {
            status: 0,
            step: "engine-lineage",
        })?;
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
    let driven = turn::poll_and_drive(
        &mut link,
        set.id,
        &session,
        admin.expose(),
        journal,
        docker,
        resource_budget,
        host_config.host.max_jobs,
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
}

/// Acquire, JIT, and start for one poll. No session create.
///
/// # Errors
///
/// Returns [`EnsureError`] when more than one job is offered, or a later step fails.
/// An uncertain acquire is not acknowledged.
pub(crate) async fn drive_offer<T, P, PF, S, F>(
    lane: &mut T,
    ctx: &Drive,
    polled: &Poll,
    journal: &Journal,
    prepare: P,
    start: S,
) -> Result<Option<Started>, EnsureError>
where
    T: velnor_runner_github::Transport + Lane,
    P: FnOnce(LaunchIdentity) -> PF,
    PF: std::future::Future<Output = Result<PreparedDind, HostError>>,
    S: FnOnce(LaunchIdentity, PreparedDind, Vec<u8>) -> F,
    F: std::future::Future<Output = Result<Started, HostError>>,
{
    drive_offer_reserved(lane, ctx, polled, journal, None, prepare, start).await
}

/// Run one offer after poll admission reserved its durable slot.
pub(crate) async fn drive_offer_reserved<T, P, PF, S, F>(
    lane: &mut T,
    ctx: &Drive,
    polled: &Poll,
    journal: &Journal,
    reservation: Option<LaunchReservation>,
    prepare: P,
    start: S,
) -> Result<Option<Started>, EnsureError>
where
    T: velnor_runner_github::Transport + Lane,
    P: FnOnce(LaunchIdentity) -> PF,
    PF: std::future::Future<Output = Result<PreparedDind, HostError>>,
    S: FnOnce(LaunchIdentity, PreparedDind, Vec<u8>) -> F,
    F: std::future::Future<Output = Result<Started, HostError>>,
{
    if matches!(steps::idle(polled), steps::Idle::Scale) {
        let Poll::Batch(batch) = polled else {
            return Ok(None);
        };
        return steps::scale_id(lane, ctx, batch, journal, prepare, start).await;
    }
    let Some((batch, request_id)) = steps::assignment(polled)? else {
        return Ok(None);
    };
    steps_acquire::launch_reserved(
        lane,
        ctx,
        batch,
        journal,
        request_id,
        reservation,
        prepare,
        start,
    )
    .await
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
    resource_budget: ResourceBudget,
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
    let mut lane = HostLane::new(link, admin, None);
    let name = runner_name(&session.session_id);
    steps::scale_unacked(
        &mut lane,
        &ctx,
        journal,
        &name,
        |identity| async move { prepare_dind_until(docker, &identity, resource_budget).await },
        |_identity, prepared, payload| async move {
            start_runner_until(docker, &prepared, &payload, None).await
        },
    )
    .await
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
    reservation: Option<LaunchReservation>,
    resource_budget: ResourceBudget,
) -> Result<Option<Started>, EnsureError> {
    let ctx = Drive {
        set_id: ready.set_id,
        queue_path: ready.path,
        queue_token: ready.session.token().to_owned(),
        admin_token: ready.admin_token.to_owned(),
    };
    let admin = link.base().to_owned();
    let mut lane = HostLane::new(link, admin, ready.queue);
    drive_offer_reserved(
        &mut lane,
        &ctx,
        ready.polled,
        journal,
        reservation,
        |identity| async move { prepare_dind_until(docker, &identity, resource_budget).await },
        |_identity, prepared, payload| async move {
            start_runner_until(docker, &prepared, &payload, None).await
        },
    )
    .await
}
