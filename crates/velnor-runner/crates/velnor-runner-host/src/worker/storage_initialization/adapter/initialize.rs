use super::{
    ConnectionJournal, CreateInterruptionOutcome, Docker, Duration, HostError, InitializationPhase,
    Instant, PendingStorageInitialization, StorageInitializationClaim, StorageInitializationImage,
    StorageInitializationLimits, StorageInitializerCreateResponse, VerifiedCreatedStorageObject,
    VerifiedStorageInitializerAwaitingHandoff, VerifiedStorageInitializerResult,
    bounded_by_deadline, create, run, start,
};

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
    let mut phase = InitializationPhase::Create;
    let mut active_claim = None;
    let mut claim_pending = false;
    let mut response_evidence = None;
    let result = bounded_by_deadline(
        deadline,
        Box::pin(async {
            claim_pending = true;
            let claimed = journal
                .claim_storage_initialization_v2(reservation_id, image, limits, metadata)
                .await?;
            claim_pending = false;
            active_claim = Some(claimed.ok_or(HostError::Journal)?);
            let claim = active_claim.as_ref().ok_or(HostError::Journal)?;
            let created = create(
                docker,
                journal,
                claim,
                image,
                limits,
                &mut phase,
                &mut response_evidence,
            )
            .await?;
            phase = InitializationPhase::CreateVerified;
            journal
                .record_storage_initializer_created(claim, &created)
                .await?;
            phase = InitializationPhase::Start;
            let (start_claim, started, attached) = start(docker, journal, claim, &created).await?;
            phase = InitializationPhase::Process;
            let (observed, attached) =
                run(docker, journal, &start_claim, &started, attached, deadline).await?;
            let result_proof =
                VerifiedStorageInitializerResult::after_validated_result(&start_claim, &observed)?;
            if let Err(error) = journal
                .record_storage_initializer_result_v2(&start_claim, &observed, &result_proof)
                .await
            {
                journal
                    .mark_storage_initialization_result_uncertain(&start_claim)
                    .await?;
                return Err(error);
            }
            let awaiting_handoff = VerifiedStorageInitializerAwaitingHandoff::after_ready(
                journal.connection_id().as_str(),
                &start_claim,
                &started,
                &result_proof,
            )?;
            phase = InitializationPhase::Handoff;
            let metadata_handoff = journal
                .record_storage_initializer_ready_held(&start_claim, &awaiting_handoff)
                .await?;
            Ok(PendingStorageInitialization::new(
                start_claim,
                started,
                attached,
                observed,
                awaiting_handoff,
                metadata_handoff,
            ))
        }),
    )
    .await;
    match result {
        Ok(Ok(pending)) => Ok(pending),
        Ok(Err(error)) => {
            if is_pre_start(phase) {
                resolve_create_interruption(
                    journal,
                    active_claim.as_ref(),
                    claim_pending,
                    reservation_id,
                    image,
                    limits,
                    phase,
                    response_evidence.as_ref(),
                )
                .await?;
            }
            Err(error)
        }
        Err(_) => {
            if is_pre_start(phase) {
                resolve_create_interruption(
                    journal,
                    active_claim.as_ref(),
                    claim_pending,
                    reservation_id,
                    image,
                    limits,
                    phase,
                    response_evidence.as_ref(),
                )
                .await?;
            } else if let Some(claim) = active_claim.as_ref() {
                match phase {
                    InitializationPhase::Create
                    | InitializationPhase::CreateDispatched
                    | InitializationPhase::CreateResponseReceived
                    | InitializationPhase::CreateVerified => {}
                    InitializationPhase::Start => {
                        journal
                            .mark_storage_initializer_start_uncertain(claim)
                            .await?;
                    }
                    InitializationPhase::Process => {
                        journal
                            .mark_storage_initialization_result_uncertain(claim)
                            .await?;
                    }
                    InitializationPhase::Handoff => {}
                }
            }
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
    claim: Option<&StorageInitializationClaim>,
    claim_pending: bool,
    reservation_id: i64,
    image: &StorageInitializationImage,
    limits: StorageInitializationLimits,
    phase: InitializationPhase,
    response: Option<&StorageInitializerCreateResponse>,
) -> Result<(), HostError> {
    if let Some(claim) = claim {
        let (outcome, response) = match phase {
            InitializationPhase::Create => (CreateInterruptionOutcome::NoRequestEntered, None),
            InitializationPhase::CreateDispatched => {
                (CreateInterruptionOutcome::RequestOutcomeUnknown, None)
            }
            InitializationPhase::CreateResponseReceived | InitializationPhase::CreateVerified => (
                CreateInterruptionOutcome::TerminalResponseReceived,
                response,
            ),
            InitializationPhase::Start
            | InitializationPhase::Process
            | InitializationPhase::Handoff => return Ok(()),
        };
        journal
            .resolve_storage_initializer_create_interruption(claim, outcome, response)
            .await
    } else if claim_pending {
        journal
            .mark_storage_initializer_claim_interrupted(reservation_id, image, limits)
            .await
            .map(|_| ())
    } else {
        Ok(())
    }
}
