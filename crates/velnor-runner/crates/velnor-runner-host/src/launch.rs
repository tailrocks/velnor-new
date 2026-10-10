//! Acquire jobs up to capacity, mint JIT, and start one paired worker each.
//!
//! The journal row is committed before acquire, JIT, and Docker. JIT bytes are
//! passed to the starter and are not written to the journal.

use std::future::Future;

use velnor_runner_github::{
    Exchange, Poll, QueueSession, SessionRequest, Transport, TransportFail,
};

use crate::ensure_product_scale_set;
use crate::error::HostError;
use crate::journal::Journal;
use crate::listen::{Link, Secret, admin_link};
use crate::reconcile::Reconcile;
use crate::scale_set::EnsureError;
use crate::worker::ResourceBudget;
use crate::worker::Started;

mod bind;
#[cfg(all(test, unix))]
mod busy_slot_tests;
mod capacity;
mod completion;
mod drive;
#[cfg(all(test, unix))]
mod effect_tests;
mod gate;
mod inspect;
#[cfg(all(test, unix))]
mod inspect_tests;
mod name_taken;
mod preflight;
mod resource_capacity;
mod resource_probe;
mod runner_dir;

pub(crate) use drive::{Drive, Lane, Rest};
pub(crate) use inspect::classify_inspect;
pub(crate) use slot::HOLDS_ROWS_SQL;
mod mint_origin;
mod pressure;
mod session;
mod slot;
mod steady;
mod steps;
#[cfg(test)]
mod subject_tests;
mod trace;
mod turn;

pub(crate) use capacity::{install_job_capacity, job_capacity};
pub(crate) use pressure::advertise as advertise_capacity;

#[cfg(test)]
pub(crate) use capacity::{
    Admit, Seat, admit, needs_running, parse_admit_target, parse_job_capacity, poll_limit,
    statistics_blocked, wide_poll_limit,
};
#[cfg(test)]
pub(crate) use name_taken::{fail_unstarted, should_ack};
#[cfg(test)]
pub(crate) use steps::{Idle, idle};
#[cfg(test)]
pub(crate) use turn::admission;

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
    resource_budget: ResourceBudget,
) -> Result<LaunchReport, EnsureError> {
    let ceiling = job_capacity();
    let capacity = preflight::run(docker, docker, journal, resource_budget, ceiling).await?;
    let _capacity = install_job_capacity(capacity.poll_header());
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
    let rest = Rest {
        owner,
        repo,
        pat,
        resource_budget: Some(resource_budget),
        static_capacity: capacity.permits_start(),
        guest_admission: drive::GuestAdmission::Unavailable,
    };
    let driven = turn::poll_and_drive(
        &mut link,
        set.id,
        &session,
        admin.expose(),
        journal,
        docker,
        rest,
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
    S: Fn(&str, &[u8], bind::Bind) -> F,
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
    rest: Rest<'_>,
) -> Result<Option<Started>, EnsureError> {
    if !rest.permits_start() {
        return Ok(None);
    }
    let population = session
        .statistics()
        .map_or(0, velnor_runner_github::Statistics::assigned_population);
    if population <= 0 {
        return Ok(None);
    }
    let resource_budget = rest.resource_budget.ok_or(EnsureError::Unexpected {
        status: 0,
        step: "resource budget",
    })?;
    let Some(permit) =
        resource_probe::start_permit(docker, journal, resource_budget, rest.guest_admission).await
    else {
        return Ok(None);
    };
    let mut ctx = Drive::from_rest(
        set_id,
        String::new(),
        session.token().to_owned(),
        admin_token.to_owned(),
        rest,
    );
    let Some((engine_id, root_digest)) = resource_probe::engine_binding(docker).await.ok() else {
        return Ok(None);
    };
    if !resource_probe::consume_permit(permit, &engine_id, &root_digest) {
        return Ok(None);
    }
    ctx.docker_engine_id = Some(engine_id);
    let admin = link.base().to_owned();
    let mut lane = HostLane {
        link,
        admin,
        queue: None,
    };
    let name = runner_name(&session.session_id);
    steps::scale_unacked(&mut lane, &ctx, journal, &name, |volume, jit, bind| {
        let volume = volume.to_owned();
        let payload = jit.to_vec();
        async move { bind::start_bound(docker, &volume, &payload, resource_budget, &bind).await }
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
    polled: &'a Poll,
}

async fn drive_ready<T>(
    lane: &mut T,
    ready: Ready<'_>,
    journal: &Journal,
    docker: &bollard::Docker,
    capacity: u32,
    rest: Rest<'_>,
) -> Result<Option<Started>, EnsureError>
where
    T: Transport + Lane,
{
    if !rest.permits_start() {
        return Ok(None);
    }
    let except = steps::mint_subject(ready.polled);
    if slot::busy_except(journal, docker, capacity, except.as_deref()).await? {
        return Ok(None);
    }
    let resource_budget = rest.resource_budget.ok_or(EnsureError::Unexpected {
        status: 0,
        step: "resource budget",
    })?;
    let Some(permit) =
        resource_probe::start_permit(docker, journal, resource_budget, rest.guest_admission).await
    else {
        return Ok(None);
    };
    let mut ctx = Drive::from_rest(
        ready.set_id,
        ready.path,
        ready.session.token().to_owned(),
        ready.admin_token.to_owned(),
        rest,
    );
    let Some((engine_id, root_digest)) = resource_probe::engine_binding(docker).await.ok() else {
        return Ok(None);
    };
    if !resource_probe::consume_permit(permit, &engine_id, &root_digest) {
        return Ok(None);
    }
    ctx.docker_engine_id = Some(engine_id);
    drive_offer(lane, &ctx, ready.polled, journal, |volume, jit, bind| {
        let volume = volume.to_owned();
        let payload = jit.to_vec();
        async move { bind::start_bound(docker, &volume, &payload, resource_budget, &bind).await }
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
    let batch = match polled {
        Poll::Batch(batch) => batch.clone(),
        Poll::Quarantined(batch) => velnor_runner_github::ParsedBatch {
            message_id: batch.message_id,
            raw_body: batch.raw_body.clone(),
            statistics: None,
            jobs: Vec::new(),
        },
        Poll::Empty => return Ok(()),
    };
    let ctx = Drive::from_rest(
        0,
        path,
        session.token().to_owned(),
        String::new(),
        Rest {
            owner: "",
            repo: "",
            pat: "",
            resource_budget: None,
            static_capacity: false,
            guest_admission: drive::GuestAdmission::Unavailable,
        },
    );
    let admin = link.base().to_owned();
    let mut lane = HostLane { link, admin, queue };
    steps::acknowledge(&mut lane, &ctx, &batch)
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

    fn use_github_api(&mut self) -> Result<(), EnsureError> {
        self.link.set_base(crate::listen::GITHUB_API)
    }
}
