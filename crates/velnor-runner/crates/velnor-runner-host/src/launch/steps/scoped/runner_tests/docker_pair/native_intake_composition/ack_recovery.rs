use super::*;

#[test]
fn acknowledged_acquire_reopens_and_rechecks_before_jit() -> Result<(), String> {
    const PRODUCTION_CALLER_STACK_BYTES: usize = 8 * 1024 * 1024;
    std::thread::Builder::new()
        .name("native-ack-recovery-production-stack".to_owned())
        .stack_size(PRODUCTION_CALLER_STACK_BYTES)
        .spawn(|| {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .map_err(|error| error.to_string())?;
            runtime.block_on(acknowledged_acquire_reopens_and_rechecks_before_jit_inner())
        })
        .map_err(|error| error.to_string())?
        .join()
        .map_err(|_| "The ACK recovery test thread stopped unexpectedly.".to_owned())?
}

async fn acknowledged_acquire_reopens_and_rechecks_before_jit_inner() -> Result<(), String> {
    let mut setup = bootstrap_start::open_composition().await?;
    assert_acknowledged_before_jit(&setup)?;
    let poll_session = setup
        .poll_session
        .take()
        .ok_or_else(|| "The positive poll session is absent.".to_owned())?;
    let (request, reservation_id, budget) = acquired_identity(&setup)?;
    let policy = setup.fixture.authority_policy.clone();
    let state_path = setup.admission_state_path().to_path_buf();
    drop(setup.outcome.take());
    let reopened = setup.fixture.reopen().await?;
    let charge = assert_acquired_row(&reopened, &request, reservation_id, &policy).await?;
    let case = ReopenedAcquire {
        state_path,
        engine: &setup.engine,
        fixture: &reopened,
        request: &request,
        reservation_id,
        budget,
        charge,
    };
    assert_missing_sample_holds_charge(&case, &policy).await?;
    assert_current_policy_recovers(&case, &policy).await?;
    let changed_policy = install_next_policy(&case, policy.generation()).await?;
    assert_policy_generation_is_current(&case, &policy, &changed_policy).await?;
    assert_eq!(poll_session.acquire_calls(), 1);
    assert_eq!(poll_session.acknowledgement_calls(), 1);
    assert_eq!(setup.api.activity_snapshot()?.jit_input_bytes, 0);
    Ok(())
}

fn assert_acknowledged_before_jit(setup: &bootstrap_start::CompositionSetup) -> Result<(), String> {
    let poll_session = setup
        .poll_session
        .as_ref()
        .ok_or_else(|| "The positive poll session is absent.".to_owned())?;
    assert_eq!(poll_session.acquire_calls(), 1);
    assert_eq!(poll_session.acknowledgement_calls(), 1);
    let activity = setup.api.activity_snapshot()?;
    assert_eq!(activity.jit_input_bytes, 0);
    assert_eq!(activity.starts, 0);
    Ok(())
}

fn acquired_identity(
    setup: &bootstrap_start::CompositionSetup,
) -> Result<
    (
        crate::journal::AssignmentIdentity,
        i64,
        crate::worker::ResourceBudget,
    ),
    String,
> {
    let Some(crate::launch::steps::ScopedProvisioningOutcome::Acquired(acquired)) =
        setup.outcome.as_ref()
    else {
        return Err("The positive poll did not retain its acquired request.".to_owned());
    };
    Ok((
        acquired.request().clone(),
        acquired.reservation_id(),
        acquired.resource_budget(),
    ))
}

async fn assert_acquired_row(
    fixture: &Fixture,
    request: &crate::journal::AssignmentIdentity,
    reservation_id: i64,
    policy: &crate::config::ApprovedJobAuthorityPolicy,
) -> Result<(u64, u64), String> {
    assert_eq!(
        fixture
            .scope
            .request_acquire_state(request)
            .await
            .map_err(|error| error.to_string())?,
        Some(crate::journal::RequestAcquireState::Acquired)
    );
    let candidate = fixture
        .scope
        .acquired_recovery_candidate(
            request,
            reservation_id,
            policy.generation(),
            policy.policy_sha256(),
        )
        .await
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "The exact acquired response is not recoverable.".to_owned())?;
    assert_eq!(candidate.request(), request);
    assert_eq!(candidate.reservation_id(), reservation_id);
    let charge = candidate.charge();
    assert_held_acquired_row(fixture, request, reservation_id, charge).await?;
    Ok(charge)
}

async fn assert_held_acquired_row(
    fixture: &Fixture,
    request: &crate::journal::AssignmentIdentity,
    reservation_id: i64,
    charge: (u64, u64),
) -> Result<(), String> {
    assert_eq!(
        fixture
            .scope
            .request_acquire_state(request)
            .await
            .map_err(|error| error.to_string())?,
        Some(crate::journal::RequestAcquireState::Acquired)
    );
    let acquired = fixture
        .scope
        .acquired_authorized_requests(64)
        .await
        .map_err(|error| error.to_string())?;
    assert!(acquired.contains(request));
    let rows = fixture
        .scope
        .physical_runner_reservations()
        .await
        .map_err(|error| error.to_string())?;
    let row = rows
        .iter()
        .find(|row| row.reservation_id == reservation_id)
        .ok_or_else(|| "The acquired reservation is absent.".to_owned())?;
    assert_eq!(
        row.reservation.state,
        crate::journal::ReservationState::Reserved
    );
    assert_eq!(
        row.jit_state,
        crate::journal::PhysicalRunnerJitState::Absent
    );
    assert_eq!(
        (row.reservation.cpu_millicores, row.reservation.memory_bytes),
        charge
    );
    assert_eq!(fixture.journal.occupied_reservation_count().await, Ok(1));
    Ok(())
}

struct ReopenedAcquire<'a> {
    state_path: std::path::PathBuf,
    engine: &'a crate::launch::SelectedEngineContext,
    fixture: &'a Fixture,
    request: &'a crate::journal::AssignmentIdentity,
    reservation_id: i64,
    budget: crate::worker::ResourceBudget,
    charge: (u64, u64),
}

impl ReopenedAcquire<'_> {
    async fn recover(
        &self,
        policy: &crate::config::ApprovedJobAuthorityPolicy,
        include_sample: bool,
    ) -> Result<crate::launch::steps::ScopedProvisioningOutcome, String> {
        let observation = include_sample
            .then(|| recovery_observation(self.engine, self.budget))
            .transpose()?;
        recover_after_ack(
            &self.state_path,
            self.engine,
            self.fixture,
            self.request,
            self.reservation_id,
            policy,
            observation,
        )
        .await
    }

    async fn assert_charge_held(&self) -> Result<(), String> {
        assert_held_acquired_row(self.fixture, self.request, self.reservation_id, self.charge).await
    }
}

async fn assert_missing_sample_holds_charge(
    case: &ReopenedAcquire<'_>,
    policy: &crate::config::ApprovedJobAuthorityPolicy,
) -> Result<(), String> {
    let outcome = case.recover(policy, false).await?;
    assert!(matches!(
        outcome,
        crate::launch::steps::ScopedProvisioningOutcome::NoStart
    ));
    case.assert_charge_held().await
}

async fn assert_current_policy_recovers(
    case: &ReopenedAcquire<'_>,
    policy: &crate::config::ApprovedJobAuthorityPolicy,
) -> Result<(), String> {
    let outcome = case.recover(policy, true).await?;
    if !matches!(
        outcome,
        crate::launch::steps::ScopedProvisioningOutcome::Recovered(_)
    ) {
        return Err("The unchanged policy did not recover the durable Acquire.".to_owned());
    }
    drop(outcome);
    case.assert_charge_held().await
}

async fn install_next_policy(
    case: &ReopenedAcquire<'_>,
    generation: u64,
) -> Result<crate::config::ApprovedJobAuthorityPolicy, String> {
    let config = crate::config::HostConfig::test_with_job_authority_policy_generation(
        &case.fixture.binding,
        generation + 1,
    )
    .map_err(|error| error.to_string())?;
    case.fixture
        .scope
        .install_job_authority_policy(&config)
        .await
        .map_err(|error| error.to_string())?;
    config
        .validated_job_authority_policy(&case.fixture.binding.connection_id)
        .map_err(|error| error.to_string())?
        .cloned()
        .ok_or_else(|| "The changed job authority policy is absent.".to_owned())
}

async fn assert_policy_generation_is_current(
    case: &ReopenedAcquire<'_>,
    old_policy: &crate::config::ApprovedJobAuthorityPolicy,
    current_policy: &crate::config::ApprovedJobAuthorityPolicy,
) -> Result<(), String> {
    let stale = case.recover(old_policy, true).await?;
    assert!(matches!(
        stale,
        crate::launch::steps::ScopedProvisioningOutcome::NoStart
    ));
    case.assert_charge_held().await?;
    let current = case.recover(current_policy, true).await?;
    if !matches!(
        current,
        crate::launch::steps::ScopedProvisioningOutcome::Recovered(_)
    ) {
        return Err("The current policy did not recover the acquired request.".to_owned());
    }
    drop(current);
    case.assert_charge_held().await
}

fn recovery_observation(
    engine: &crate::launch::SelectedEngineContext,
    budget: crate::worker::ResourceBudget,
) -> Result<crate::launch::RecoveryObservation, String> {
    const MEMORY_BYTES: u64 = 32 * 1024 * 1024 * 1024;
    crate::launch::StartPermit::test_fixture_with_engine_and_available_memory(
        &"a".repeat(32),
        engine.engine_id(),
        MEMORY_BYTES,
    )
    .into_test_recovery_observation()
    .filter(|observation| observation.resource_budget() == budget)
    .ok_or_else(|| "The test recovery sample does not match the stored charge.".to_owned())
}

async fn recover_after_ack(
    admission_state: &std::path::Path,
    engine: &crate::launch::SelectedEngineContext,
    fixture: &Fixture,
    request: &crate::journal::AssignmentIdentity,
    reservation_id: i64,
    policy: &crate::config::ApprovedJobAuthorityPolicy,
    observation: Option<crate::launch::RecoveryObservation>,
) -> Result<crate::launch::steps::ScopedProvisioningOutcome, String> {
    crate::launch::recover_acquired_scoped_offer_with_test_observation(
        &fixture.binding,
        policy,
        admission_state,
        &fixture.scope,
        request,
        reservation_id,
        engine,
        observation,
    )
    .await
    .map_err(|error| error.to_string())
}
