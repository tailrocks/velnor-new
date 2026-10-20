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
use crate::worker::{ResourceBudget, Started};

mod admission_gate;
mod bind;
pub(crate) mod bootstrap;
#[cfg(all(test, unix))]
mod busy_slot_tests;
mod capacity;
mod completion;
mod drive;
#[cfg(all(test, unix))]
mod effect_tests;
mod engine_context;
pub(crate) mod execution_authorization;
mod gate;
mod inspect;
#[cfg(all(test, unix))]
mod inspect_tests;
mod name_taken;
mod resource_probe;
mod runner_dir;
pub(crate) mod scoped_runtime;

pub(crate) use admission_gate::lock_connection;
#[cfg(test)]
pub(crate) use drive::acquire_scoped_offer;
#[cfg(test)]
pub(crate) use drive::authorize_positive_offer_with;
pub(crate) use drive::{
    DeferredAuthorizedOffer, DeferredAuthorizedOffers, Drive, GuestAdmission, Lane,
    PositiveOfferAuthorization, Rest, ScopedAdmissionInput, acquire_authorized_scoped_offer,
    authorize_deferred_offers, authorize_positive_offer, recover_acquired_scoped_offer,
};
pub(crate) use engine_context::SelectedEngineContext;
pub(crate) use inspect::classify_inspect;
pub(crate) use resource_probe::StartObservationWitness;
pub(crate) use resource_probe::StartPermit;
pub(crate) use resource_probe::VerifiedProbeCleanup;
pub(crate) use resource_probe::build_pin_status;
pub(crate) use resource_probe::publication::VerifiedExecutionPublicationCleanup;
pub(crate) use resource_probe::run_doctor_probe;
pub(crate) use resource_probe::start_permit;
pub(crate) use resource_probe::{RecoveryObservation, ReleaseReadSecret};
pub(crate) use session::{
    CompletedHeaderPoll, DeletedSession204, ReturnedSessionRefreshTransportError,
    SessionDeleteOutcome, SessionRefreshStartupAuthority, VerifiedSessionRefresh,
    delete_removed_connection_session, open_connection_session,
};
pub(crate) use slot::HOLDS_ROWS_SQL;
#[cfg(test)]
pub(crate) use turn::Acknowledgement;
#[cfg(test)]
pub(crate) use turn::classify_owned_terminal_available;
#[cfg(test)]
pub(crate) use turn::test_positive_poll_once_with_response;
#[cfg(test)]
pub(crate) use turn::test_scoped_intake;
pub(crate) use turn::{
    CursorDisposition, PositiveAdmission, PositivePollOutcome, admit_when_ready, positive_poll_once,
};
#[cfg(test)]
pub(crate) use turn::{IntakeDecision, finish_positive_poll_with_ack};
pub(crate) use turn::{
    ScopedPollOutcome, Settlement, ZeroHeaderMode, poll_scoped_connection_once_with_mode,
};
mod mint_origin;
#[cfg(test)]
mod pressure;
mod session;
mod slot;
#[cfg(all(test, unix))]
#[path = "launch/source_order_tests.rs"]
mod source_order_tests;
pub(crate) mod steady;
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

pub(crate) async fn startup_storage_profile(
    docker: &bollard::Docker,
    journal: &Journal,
    budget: ResourceBudget,
    max_jobs: u32,
    secret: &ReleaseReadSecret,
    images: &VerifiedWorkerImages,
) -> Option<crate::journal::StorageProfile> {
    let permit = resource_probe::start_permit(docker, journal, budget, secret).await?;
    (permit.resource_budget() == budget)
        .then(|| permit.storage_profile(images, budget, max_jobs))
        .flatten()
}

pub(crate) fn retained_tmpfs_memory_bytes() -> Option<u64> {
    resource_probe::retained_tmpfs_memory_bytes()
}

/// Run one scoped queue poll with the supplied durable zero-header allocation.
pub(crate) async fn poll_scoped_connection_once(
    link: &mut Link,
    set_id: i64,
    session: &mut QueueSession,
    scoped: &ConnectionJournal,
    session_intent_id: i64,
    allocation: &HeaderAllocation,
    cursor: i64,
) -> Result<ScopedPollOutcome, EnsureError> {
    turn::poll_scoped_connection_once(
        link,
        set_id,
        session,
        scoped,
        session_intent_id,
        allocation,
        cursor,
    )
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
pub(crate) use steps::run_native_quarantine_composition;
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
            authority_policy: None,
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
    let mut lane = HostLane::new(link, admin, queue);
    steps::acknowledge(&mut lane, &ctx, &batch)
}

pub(crate) struct HostLane<'a> {
    link: &'a mut Link,
    admin: String,
    queue: Option<String>,
}

impl<'a> HostLane<'a> {
    /// Bind one queue callback to its saved admin and queue origins.
    pub(crate) fn new(link: &'a mut Link, admin: String, queue: Option<String>) -> Self {
        Self { link, admin, queue }
    }
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

#[cfg(test)]
pub(crate) use completion::reconcile_for_test;
