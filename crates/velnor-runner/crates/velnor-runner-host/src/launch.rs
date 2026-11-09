// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_runner_github::{
    Exchange, Poll, QueueSession, SessionRequest, Transport, TransportFail,
};

use crate::journal::{ConnectionJournal, HeaderAllocation, Journal};
use crate::listen::Link;
use crate::scale_set::EnsureError;
use crate::worker::ResourceBudget;

mod admission_gate;
pub(crate) mod bootstrap;
mod completion;
mod drive;
mod engine_context;
pub(crate) mod execution_authorization;
mod gate;
mod inspect;

pub(crate) mod jit_conflict;
mod name_taken;
mod resource_probe;
pub(crate) use resource_probe::{
    ProjectedCandidate, inspect_projected_candidate, qualify_mac_family,
};
pub(crate) mod retirement;
pub(crate) mod scoped_runtime;

pub(crate) use admission_gate::lock_connection;
pub(in crate::launch) use completion::{
    remove_owned_resources_for_jit_conflict, remove_owned_resources_for_retirement,
};

pub(crate) use drive::{
    AcquiredRecoveryContext, DeferredAuthorizedOffer, DeferredAuthorizedOffers, Drive, Lane,
    PositiveOfferAuthorization, Rest, ScopedAdmissionInput, acquire_authorized_scoped_offer,
    authorize_deferred_offers, authorize_positive_offer, recover_acquired_scoped_offer,
};
pub(crate) use engine_context::SelectedEngineContext;

pub(crate) use pressure::{adjust_with_sample, advertise};
pub(crate) use resource_probe::FreshPressureSample;

pub(crate) use resource_probe::ReadinessSample;
pub(crate) use resource_probe::StartObservationWitness;
pub(crate) use resource_probe::StartPermit;
pub(crate) use resource_probe::VerifiedProbeCleanup;
pub(crate) use resource_probe::build_pin_status;
pub(crate) use resource_probe::collect_readiness_sample;

pub(crate) use resource_probe::publication::VerifiedExecutionPublicationCleanup;
pub(crate) use resource_probe::run_doctor_probe;
pub(crate) use resource_probe::start_permit;

pub(in crate::launch) use resource_probe::{PreJitCapacityObservation, measure_pre_jit_capacity};
pub(crate) use resource_probe::{RecoveryObservation, ReleaseReadSecret};
pub(crate) use session::{
    CompletedHeaderPoll, DeletedSession204, ReturnedSessionRefreshTransportError,
    SessionDeleteOutcome, SessionRefreshStartupAuthority, VerifiedSessionRefresh,
    delete_removed_connection_session, open_connection_session,
};

pub(crate) use turn::{
    CursorDisposition, PositiveAdmission, PositivePollContext, PositivePollOutcome,
    admit_when_ready, positive_poll_once_with_pressure,
};

pub(crate) use turn::{
    ScopedPollContext, ScopedPollOutcome, Settlement, ZeroHeaderMode,
    poll_scoped_connection_once_with_mode,
};
mod pressure;
mod session;

pub(crate) mod steady;
pub(crate) mod steps;

mod trace;
mod turn;
pub(crate) mod verified_images;

pub use bootstrap::{BootstrapContainerRole, runner_pair_environment};
pub use completion::CompletionWorker;
pub use execution_authorization::verify_persisted_execution_source_role;
pub(crate) use gate::{gate_line, reconcile_gate};
pub(crate) use resource_probe::prepare_worker_images;
pub use resource_probe::{
    AcquiredRegistryImage, DaemonSelection, ImageObservation, ImageReference, ImageSession,
    RegistryImageSpecification, RegistryProjection, WorkerImage, acquire_registry_image,
    verify_worker_attestation_manifest, verify_worker_build_source, verify_worker_runtime_config,
};
pub use steps::resume_deferred_offer_acquire_with_transport;
pub(crate) use verified_images::{VerifiedImageIdentity, VerifiedWorkerImages};

pub(crate) async fn startup_storage_profile(
    docker: &bollard::Docker,
    journal: &Journal,
    budget: ResourceBudget,
    max_jobs: u32,
    secret: &ReleaseReadSecret,
    images: &VerifiedWorkerImages,
    endpoint: &str,
) -> Option<crate::journal::StorageProfile> {
    let permit = resource_probe::start_permit(docker, journal, budget, secret, endpoint).await?;
    (permit.resource_budget() == budget)
        .then(|| permit.storage_profile(images, budget, max_jobs))
        .flatten()
}

pub(crate) fn retained_tmpfs_memory_bytes() -> Option<u64> {
    resource_probe::retained_tmpfs_memory_bytes()
}

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
            connection: None,
            authority_policy: None,
            host_journal: None,
            scoped_journal: None,
            allocated_header: 0,
            source_authorized: false,
            admission_state: None,
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
}
