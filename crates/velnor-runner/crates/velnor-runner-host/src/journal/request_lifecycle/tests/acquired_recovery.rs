use super::helpers::{Scratch, binding, created_scope, receipt};
use crate::error::HostError;
use crate::journal::{
    AssignedAcquireResult, AssignmentKey, CapacityLimits, Journal,
    RequestPhysicalReservationResult, ReservationRequest, SourceAuthorizationResult,
    fixture_storage_provisioning_admission,
};
use crate::launch::StartPermit;

const ENGINE_ID: &str = super::helpers::TEST_ENGINE_ID;
const JOB_MEMORY: u64 = 8_589_934_592;

pub(super) struct AcquiredCandidateFixture {
    _scratch: Scratch,
    _journal: Journal,
    pub(super) scope: crate::journal::ConnectionJournal,
    pub(super) request: crate::journal::AssignmentIdentity,
    pub(super) set_id: i64,
    pub(super) binding: crate::config::RepositoryConnection,
    pub(super) candidate: crate::journal::AcquiredRecoveryCandidate,
}

pub(super) async fn acquired_candidate_fixture() -> Result<AcquiredCandidateFixture, HostError> {
    const REQUEST_ID: i64 = 811;
    const SET_ID: i64 = 501;
    let scratch = Scratch::new()?;
    let path = scratch.file();
    let journal = Journal::open(&path).await?;
    journal
        .configure_capacity(CapacityLimits {
            jobs: 1,
            cpu_millicores: 2_000,
            memory_bytes: 25_769_803_776,
        })
        .await?;
    let binding = binding("essential", 101)?;
    let scope = created_scope(&journal, &binding, SET_ID).await?;
    let config = crate::config::HostConfig::test_with_job_authority_policy(&binding)?;
    scope.install_job_authority_policy(&config).await?;

    let request = scope
        .assignment_identity(AssignmentKey::AssignedRequest {
            request_id: REQUEST_ID,
        })
        .await?;
    let source = receipt(&scope, REQUEST_ID, REQUEST_ID + 100, 101).await?;
    assert_eq!(
        scope.record_source_authorization(&request, &source).await?,
        SourceAuthorizationResult::Recorded
    );

    let physical = scope
        .assignment_identity(AssignmentKey::Remote {
            kind: "runner".to_owned(),
            subject: "a".repeat(32),
        })
        .await?;
    let generation = "a".repeat(32);
    let permit = StartPermit::test_fixture_with_engine_id(&generation, ENGINE_ID);
    let root_digest = permit.docker_root_digest().to_owned();
    let admission = fixture_storage_provisioning_admission(&permit)?;
    let result = scope
        .try_reserve_and_claim_request_acquire(
            ReservationRequest {
                assignment: &physical,
                engine_id: ENGINE_ID,
                docker_root_digest: &root_digest,
                cpu_millicores: 2_000,
                memory_bytes: JOB_MEMORY,
                sample_generation_id: &generation,
            },
            permit,
            admission,
            &request,
        )
        .await?;
    let RequestPhysicalReservationResult::AcquireClaimed {
        reservation_id,
        intent_id,
    } = result
    else {
        return Err(HostError::Journal);
    };
    assert!(reservation_id > 0 && intent_id > 0);
    scope
        .record_request_acquire(&request, AssignedAcquireResult::Returned(vec![REQUEST_ID]))
        .await?;

    let policy = config
        .validated_job_authority_policy(scope.connection_id())?
        .ok_or(HostError::Identity)?;
    let candidate = scope
        .acquired_recovery_candidate(
            &request,
            reservation_id,
            policy.generation(),
            policy.policy_sha256(),
        )
        .await?
        .ok_or(HostError::Journal)?;
    Ok(AcquiredCandidateFixture {
        _scratch: scratch,
        _journal: journal,
        scope,
        request,
        set_id: SET_ID,
        binding,
        candidate,
    })
}

#[tokio::test]
async fn acquired_candidate_uses_exact_active_request_and_held_reservation() -> Result<(), HostError>
{
    let fixture = acquired_candidate_fixture().await?;
    let candidate = &fixture.candidate;
    assert_eq!(candidate.request(), &fixture.request);
    assert!(candidate.reservation_id() > 0);
    assert!(candidate.intent_id() > 0);
    assert_eq!(candidate.charge(), (2_000, JOB_MEMORY));
    Ok(())
}

#[tokio::test]
async fn recovery_jit_claim_rejects_a_policy_changed_after_acquire() -> Result<(), HostError> {
    let fixture = acquired_candidate_fixture().await?;
    let candidate = &fixture.candidate;
    let nonce = candidate
        .runner_name()
        .strip_prefix('v')
        .ok_or(HostError::Identity)?
        .to_owned();
    super::helpers::provision_acquired_physical_intent(
        super::helpers::provisioning::ProvisioningFixture {
            scope: &fixture.scope,
            engine_id: candidate.engine_id().to_owned(),
            assignment: candidate.physical_assignment().clone(),
            nonce,
            reservation_id: candidate.reservation_id(),
            intent_id: candidate.intent_id(),
            docker_root_digest: candidate.docker_root_digest().to_owned(),
            sample_generation_id: candidate.sample_generation_id().to_owned(),
            filesystem_instance_digest: "a".repeat(64),
        },
    )
    .await?;
    let changed = crate::config::HostConfig::test_with_job_authority_policy_generation(
        &fixture.binding,
        candidate.policy_generation() + 1,
    )?;
    fixture.scope.install_job_authority_policy(&changed).await?;

    assert!(
        fixture
            .scope
            .claim_runner_jit_for_request(
                candidate.intent_id(),
                candidate.reservation_id(),
                fixture.request.request_id().ok_or(HostError::Identity)?,
                candidate.runner_name(),
                candidate.policy_generation(),
                candidate.policy_digest(),
            )
            .await?
            .is_none()
    );
    let rows = fixture.scope.physical_runner_reservations().await?;
    let row = rows
        .iter()
        .find(|row| row.reservation_id == candidate.reservation_id())
        .ok_or(HostError::Journal)?;
    assert_eq!(
        row.reservation.state,
        crate::journal::ReservationState::Reserved
    );
    assert_eq!(
        row.jit_state,
        crate::journal::PhysicalRunnerJitState::Absent
    );
    Ok(())
}
