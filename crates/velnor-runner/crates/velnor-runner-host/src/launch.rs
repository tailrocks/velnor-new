//! Acquire jobs up to capacity, mint JIT, and start one paired worker each.
//!
//! The journal row is committed before acquire, JIT, and Docker. JIT bytes are
//! passed to the starter and are not written to the journal.

use std::future::Future;

use velnor_runner_github::{
    Exchange, Poll, QueueSession, SessionRequest, Transport, TransportFail,
};

use crate::error::HostError;
use crate::journal::{ConnectionJournal, HeaderAllocation, Journal};
use crate::listen::Link;
use crate::scale_set::EnsureError;
use crate::worker::Started;

mod admission_gate;
mod bind;
#[cfg(all(test, unix))]
mod busy_slot_tests;
mod capacity;
mod completion;
mod drive;
#[cfg(all(test, unix))]
mod effect_tests;
mod engine_context;
mod gate;
mod inspect;
#[cfg(all(test, unix))]
mod inspect_tests;
mod name_taken;
#[cfg(test)]
mod preflight;
mod resource_capacity;
mod resource_probe;
mod runner_dir;

pub(crate) use admission_gate::lock_connection;
pub(crate) use drive::{Drive, Lane, Rest};
pub(crate) use engine_context::SelectedEngineContext;
pub(crate) use inspect::classify_inspect;
pub(crate) use resource_probe::ReleaseReadSecret;
pub(crate) use resource_probe::StartObservationWitness;
pub(crate) use resource_probe::StartPermit;
pub(crate) use resource_probe::VerifiedProbeCleanup;
pub(crate) use resource_probe::build_pin_status;
pub(crate) use session::{
    CompletedHeaderPoll, DeletedSession204, ReturnedSessionRefreshTransportError,
    SessionDeleteOutcome, SessionRefreshStartupAuthority, VerifiedSessionRefresh,
    delete_removed_connection_session, open_connection_session,
};
pub(crate) use slot::HOLDS_ROWS_SQL;
pub(crate) use turn::ScopedPollOutcome;
mod mint_origin;
mod pressure;
mod session;
mod slot;
#[cfg(all(test, unix))]
#[path = "launch/source_order_tests.rs"]
mod source_order_tests;
mod steady;
pub(crate) mod steps;
#[cfg(test)]
mod subject_tests;
mod trace;
mod turn;
pub(crate) mod verified_images;

pub(crate) use capacity::install_job_capacity;
#[cfg(test)]
pub(crate) use capacity::job_capacity;
pub(crate) use completion::CompletionWorker;
pub(crate) use gate::{gate_line, reconcile_gate};
pub(crate) use resource_probe::prepare_worker_images;
pub(crate) use verified_images::{VerifiedImageIdentity, VerifiedWorkerImages};

/// Run one scoped queue poll with the supplied durable zero-header allocation.
pub(crate) async fn poll_scoped_connection_once(
    link: &mut Link,
    set_id: i64,
    session: &mut QueueSession,
    scoped: &ConnectionJournal,
    session_intent_id: i64,
    allocation: &HeaderAllocation,
) -> Result<ScopedPollOutcome, EnsureError> {
    turn::poll_scoped_connection_once(link, set_id, session, scoped, session_intent_id, allocation)
        .await
}

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
    let (connection, images, selected_engine, guest_image) = match (
        rest.connection,
        rest.worker_images,
        rest.selected_engine,
        rest.guest_image,
    ) {
        (Some(connection), Some(images), Some(engine), Some(guest_image))
            if engine.matches_images(images) =>
        {
            (connection, images, engine, guest_image)
        }
        _ => return Ok(None),
    };
    let mut ctx = Drive::from_rest(
        ready.set_id,
        ready.path,
        ready.session.token().to_owned(),
        ready.admin_token.to_owned(),
        rest,
    );
    steps::authorize_poll(&mut ctx, ready.polled).await?;
    let except = steps::mint_subject(ready.polled);
    if slot::busy_except(journal, docker, capacity, except.as_deref()).await? {
        return Ok(None);
    }
    let resource_budget = rest.resource_budget.ok_or(EnsureError::Unexpected {
        status: 0,
        step: "resource budget",
    })?;
    let Some(release_read_secret) = rest.release_read_secret else {
        return Ok(None);
    };
    let Some(permit) =
        resource_probe::start_permit(docker, journal, resource_budget, release_read_secret).await
    else {
        return Ok(None);
    };
    let Some((engine_id, root_digest)) = resource_probe::engine_binding(docker).await.ok() else {
        return Ok(None);
    };
    if permit.engine_id() != engine_id
        || permit.docker_root_digest() != root_digest
        || engine_id != selected_engine.engine_id()
        || root_digest != selected_engine.docker_root_digest()
    {
        return Ok(None);
    }
    ctx.docker_engine_id = Some(engine_id);
    steps::launch_scoped(
        lane,
        &ctx,
        ready.polled,
        permit,
        None,
        docker,
        selected_engine,
        images,
        |volume, jit, bind, admission| {
            let volume = volume.to_owned();
            let payload = jit.to_vec();
            async move {
                bind::start_bound(
                    docker,
                    &volume,
                    &payload,
                    resource_budget,
                    connection,
                    images,
                    selected_engine,
                    guest_image,
                    admission,
                    &bind,
                )
                .await
            }
        },
    )
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
            connection: None,
            host_journal: None,
            release_read_secret: None,
            scoped_journal: None,
            allocated_header: 0,
            source_authorized: false,
            admission_state: None,
            worker_images: None,
            selected_engine: None,
            guest_image: None,
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
