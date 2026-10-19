use std::time::Duration;

use bollard::{Docker, container::AttachContainerResults};
use tokio::time::Instant;

use crate::{
    error::HostError,
    journal::{ConnectionJournal, CreateInterruptionOutcome, StorageInitializationClaim},
    launch::bootstrap::VerifiedCreatedStorageObject,
    worker::storage_initialization::{
        ObservedStorageInitialization, PendingStorageInitialization, StorageInitializationImage,
        StorageInitializationLimits,
    },
};

use super::super::{
    process::run,
    proof::{
        StorageInitializerCreateResponse, VerifiedStorageInitializerAwaitingHandoff,
        VerifiedStorageInitializerResult, VerifiedStorageInitializerStarted,
    },
};
use super::{InitializationPhase, bounded_by_deadline, create, recovery::recover_absent, start};

struct InitializationContext<'a> {
    docker: &'a Docker,
    journal: &'a ConnectionJournal,
    reservation_id: i64,
    image: &'a StorageInitializationImage,
    limits: StorageInitializationLimits,
    metadata: &'a VerifiedCreatedStorageObject,
    deadline: Instant,
    phase: InitializationPhase,
    active_claim: Option<StorageInitializationClaim>,
    claim_pending: bool,
    response_evidence: Option<StorageInitializerCreateResponse>,
}

impl InitializationContext<'_> {
    async fn run(&mut self) -> Result<PendingStorageInitialization, HostError> {
        self.claim_pending = true;
        let claimed = self
            .journal
            .claim_storage_initialization_v2(
                self.reservation_id,
                self.image,
                self.limits,
                self.metadata,
            )
            .await?;
        self.claim_pending = false;
        self.active_claim = Some(claimed.ok_or(HostError::Journal)?);
        let claim = self.active_claim.as_ref().ok_or(HostError::Journal)?;
        let created = create(
            self.docker,
            self.journal,
            claim,
            self.image,
            self.limits,
            &mut self.phase,
            &mut self.response_evidence,
        )
        .await?;
        self.phase = InitializationPhase::CreateVerified;
        self.journal
            .record_storage_initializer_created(claim, &created)
            .await?;
        self.phase = InitializationPhase::Start;
        let (start_claim, started, attached) =
            start(self.docker, self.journal, claim, &created).await?;
        self.phase = InitializationPhase::Process;
        let (observed, attached) = run(
            self.docker,
            self.journal,
            &start_claim,
            &started,
            attached,
            self.deadline,
        )
        .await?;
        let result =
            VerifiedStorageInitializerResult::after_validated_result(&start_claim, &observed)?;
        if let Err(error) = self
            .journal
            .record_storage_initializer_result_v2(&start_claim, &observed, &result)
            .await
        {
            self.journal
                .mark_storage_initialization_result_uncertain(&start_claim)
                .await?;
            return Err(error);
        }
        self.ready_pending(start_claim, started, attached, observed, result)
            .await
    }

    async fn ready_pending(
        &mut self,
        claim: StorageInitializationClaim,
        started: VerifiedStorageInitializerStarted,
        attached: AttachContainerResults,
        observed: ObservedStorageInitialization,
        result: VerifiedStorageInitializerResult,
    ) -> Result<PendingStorageInitialization, HostError> {
        let awaiting = VerifiedStorageInitializerAwaitingHandoff::after_ready(
            self.journal.connection_id().as_str(),
            &claim,
            &started,
            &result,
        )?;
        self.phase = InitializationPhase::Handoff;
        let metadata_handoff = self
            .journal
            .record_storage_initializer_ready_held(&claim, &awaiting)
            .await?;
        Ok(PendingStorageInitialization::new(
            claim,
            started,
            attached,
            observed,
            awaiting,
            metadata_handoff,
        ))
    }

    async fn resolve_failure(&self) -> Result<(), HostError> {
        if is_pre_start(self.phase) {
            resolve_create_interruption(
                self.journal,
                CreateInterruption {
                    claim: self.active_claim.as_ref(),
                    claim_pending: self.claim_pending,
                    reservation_id: self.reservation_id,
                    image: self.image,
                    limits: self.limits,
                    phase: self.phase,
                    response: self.response_evidence.as_ref(),
                },
            )
            .await?;
        } else if let Some(claim) = self.active_claim.as_ref() {
            match self.phase {
                InitializationPhase::Start => {
                    self.journal
                        .mark_storage_initializer_start_uncertain(claim)
                        .await?;
                }
                InitializationPhase::Process => {
                    self.journal
                        .mark_storage_initialization_result_uncertain(claim)
                        .await?;
                }
                InitializationPhase::Create
                | InitializationPhase::CreateDispatched
                | InitializationPhase::CreateResponseReceived
                | InitializationPhase::CreateVerified
                | InitializationPhase::Handoff => {}
            }
        }
        Ok(())
    }

    async fn recover_pre_start_abort(&self) -> Result<(), HostError> {
        if !matches!(
            self.phase,
            InitializationPhase::Create | InitializationPhase::CreateVerified
        ) || Instant::now() >= self.deadline
        {
            return Ok(());
        }
        if self
            .journal
            .storage_initializer_abort_candidate(self.reservation_id)
            .await?
            .is_none()
        {
            return Ok(());
        }
        let remaining = self.deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Ok(());
        }
        recover_absent(self.docker, self.journal, self.reservation_id, remaining).await
    }
}

/// Set the owner and mode of the Runner home volume and Work volume for one job.
pub(crate) async fn initialize(
    docker: &Docker,
    journal: &ConnectionJournal,
    reservation_id: i64,
    image: &StorageInitializationImage,
    limits: StorageInitializationLimits,
    timeout: Duration,
    metadata: &VerifiedCreatedStorageObject,
) -> Result<PendingStorageInitialization, HostError> {
    if timeout.is_zero() || timeout > Duration::from_secs(30) {
        return Err(HostError::DockerTimeout);
    }
    let deadline = Instant::now()
        .checked_add(timeout)
        .ok_or(HostError::DockerTimeout)?;
    let mut context = InitializationContext {
        docker,
        journal,
        reservation_id,
        image,
        limits,
        metadata,
        deadline,
        phase: InitializationPhase::Create,
        active_claim: None,
        claim_pending: false,
        response_evidence: None,
    };
    match bounded_by_deadline(deadline, Box::pin(context.run())).await {
        Ok(Ok(pending)) => Ok(pending),
        Ok(Err(error)) => {
            context.resolve_failure().await?;
            context.recover_pre_start_abort().await?;
            Err(error)
        }
        Err(_) => {
            context.resolve_failure().await?;
            Err(HostError::DockerTimeout)
        }
    }
}

fn is_pre_start(phase: InitializationPhase) -> bool {
    matches!(
        phase,
        InitializationPhase::Create
            | InitializationPhase::CreateDispatched
            | InitializationPhase::CreateResponseReceived
            | InitializationPhase::CreateVerified
    )
}

async fn resolve_create_interruption(
    journal: &ConnectionJournal,
    interrupted: CreateInterruption<'_>,
) -> Result<(), HostError> {
    if let Some(claim) = interrupted.claim {
        let (outcome, response) = match interrupted.phase {
            InitializationPhase::Create => (CreateInterruptionOutcome::NoRequestEntered, None),
            InitializationPhase::CreateDispatched => {
                (CreateInterruptionOutcome::RequestOutcomeUnknown, None)
            }
            InitializationPhase::CreateResponseReceived | InitializationPhase::CreateVerified => (
                CreateInterruptionOutcome::TerminalResponseReceived,
                interrupted.response,
            ),
            InitializationPhase::Start
            | InitializationPhase::Process
            | InitializationPhase::Handoff => return Ok(()),
        };
        journal
            .resolve_storage_initializer_create_interruption(claim, outcome, response)
            .await
    } else if interrupted.claim_pending {
        journal
            .mark_storage_initializer_claim_interrupted(
                interrupted.reservation_id,
                interrupted.image,
                interrupted.limits,
            )
            .await
            .map(|_| ())
    } else {
        Ok(())
    }
}

struct CreateInterruption<'a> {
    claim: Option<&'a StorageInitializationClaim>,
    claim_pending: bool,
    reservation_id: i64,
    image: &'a StorageInitializationImage,
    limits: StorageInitializationLimits,
    phase: InitializationPhase,
    response: Option<&'a StorageInitializerCreateResponse>,
}
