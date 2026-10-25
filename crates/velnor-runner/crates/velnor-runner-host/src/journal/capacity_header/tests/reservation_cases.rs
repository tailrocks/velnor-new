use super::*;

#[test]
fn exact_reservation_is_idempotent_and_zero_is_a_valid_header() -> TestResult {
    let mut ledger = HeaderLedger::new(3)?;
    let zero = reserve(&mut ledger, connection("essential")?, 10, "session-a", 0)?;
    let repeated = reserve(&mut ledger, connection("essential")?, 10, "session-a", 0)?;

    assert_eq!(zero, repeated);
    assert_eq!(ledger.charged_capacity(), 0);
    Ok(())
}

#[test]
fn fairness_cursor_uses_latest_positive_generation_including_released_records() -> TestResult {
    let mut ledger = HeaderLedger::new(2)?;
    assert_eq!(ledger.fairness_cursor(), None);

    let first = reserve(&mut ledger, connection("alpha")?, 10, "session-a", 1)?;
    assert_eq!(ledger.fairness_cursor(), Some(connection("alpha")?));
    let zero = reserve(&mut ledger, connection("beta")?, 20, "session-b", 0)?;
    assert_eq!(ledger.fairness_cursor(), Some(connection("alpha")?));

    let zero_poll = ledger.begin_zero_poll(&zero)?;
    ledger.finish_poll(&zero_poll, PollOutcome::Response)?;
    let zero_delete = ledger.begin_delete(&zero)?;
    ledger.finish_delete(&zero_delete)?;
    assert_eq!(ledger.fairness_cursor(), Some(connection("alpha")?));

    let positive_poll = ledger.begin_poll(&first)?;
    ledger.finish_poll(&positive_poll, PollOutcome::Response)?;
    let zero_poll = ledger.begin_zero_poll(&first)?;
    ledger.finish_poll(&zero_poll, PollOutcome::Response)?;
    let deletion = ledger.begin_delete(&first)?;
    ledger.finish_delete(&deletion)?;
    assert_eq!(ledger.fairness_cursor(), Some(connection("alpha")?));

    let mut restored = HeaderLedger::restore(ledger.snapshot())?;
    let newer_zero = reserve(&mut restored, connection("beta")?, 20, "session-c", 0)?;
    assert_eq!(restored.fairness_cursor(), Some(connection("alpha")?));
    let zero_poll = restored.begin_zero_poll(&newer_zero)?;
    restored.finish_poll(&zero_poll, PollOutcome::Response)?;
    let zero_delete = restored.begin_delete(&newer_zero)?;
    restored.finish_delete(&zero_delete)?;
    assert_eq!(restored.fairness_cursor(), Some(connection("alpha")?));
    let newer_positive = reserve(&mut restored, connection("beta")?, 20, "session-d", 1)?;
    assert_eq!(restored.fairness_cursor(), Some(connection("beta")?));

    let round_trip = HeaderLedger::restore(restored.snapshot())?;
    assert_eq!(round_trip.fairness_cursor(), Some(connection("beta")?));
    assert_eq!(newer_zero.target(), 0);
    assert_eq!(newer_positive.target(), 1);
    Ok(())
}

#[test]
fn aggregate_capacity_is_reserved_before_a_second_connection_can_poll() -> TestResult {
    let mut ledger = HeaderLedger::new(3)?;
    reserve(&mut ledger, connection("essential")?, 10, "session-a", 2)?;
    reserve(&mut ledger, connection("chainargos")?, 20, "session-b", 1)?;

    assert_eq!(ledger.charged_capacity(), 3);
    assert_eq!(
        reserve(&mut ledger, connection("third")?, 30, "session-c", 1),
        Err(HeaderLedgerError::OverBudget)
    );
    assert_eq!(ledger.charged_capacity(), 3);
    Ok(())
}

#[test]
fn retry_reuses_charge_but_uncertainty_blocks_target_or_session_replacement() -> TestResult {
    let mut ledger = HeaderLedger::new(4)?;
    let allocation = reserve(&mut ledger, connection("essential")?, 10, "session-a", 2)?;
    let request = ledger.begin_poll(&allocation)?;
    ledger.finish_poll(&request, PollOutcome::Unknown)?;

    let retry = reserve(&mut ledger, connection("essential")?, 10, "session-a", 2)?;
    assert_eq!(retry, allocation);
    assert_eq!(ledger.charged_capacity(), 2);
    assert_eq!(
        reserve(&mut ledger, connection("essential")?, 10, "session-a", 1),
        Err(HeaderLedgerError::TargetChanged)
    );
    assert_eq!(
        reserve(&mut ledger, connection("essential")?, 10, "session-b", 2),
        Err(HeaderLedgerError::SessionStillCharged)
    );
    assert_eq!(ledger.charged_capacity(), 2);
    Ok(())
}

#[test]
fn only_one_poll_is_in_flight_per_exact_session_generation() -> TestResult {
    let mut ledger = HeaderLedger::new(2)?;
    let allocation = reserve(&mut ledger, connection("essential")?, 10, "session-a", 1)?;
    let first = ledger.begin_poll(&allocation)?;

    assert_eq!(
        ledger.begin_poll(&allocation),
        Err(HeaderLedgerError::PollAlreadyInFlight)
    );
    ledger.finish_poll(&first, PollOutcome::Response)?;
    assert_eq!(ledger.charged_capacity(), 1);
    Ok(())
}

#[test]
fn source_adapter_identity_checks_bind_allocation_poll_and_delete() -> TestResult {
    let mut ledger = HeaderLedger::new(2)?;
    let connection_id = connection("essential")?;
    let allocation = reserve(&mut ledger, connection_id.clone(), 10, "session-a", 1)?;

    assert_eq!(allocation.connection_id(), &connection_id);
    assert_eq!(allocation.scale_set_id(), 10);
    assert_eq!(allocation.session_id(), "session-a");
    assert_eq!(allocation.target(), 1);
    assert!(allocation.matches_session(&connection_id, 10, "session-a"));

    let request = ledger.begin_poll(&allocation)?;
    assert!(request.matches_session(&connection_id, 10, "session-a"));
    assert!(!request.matches_session(&connection_id, 10, "session-b"));
    ledger.finish_poll(&request, PollOutcome::Response)?;

    let zero = ledger.begin_zero_poll(&allocation)?;
    ledger.finish_poll(&zero, PollOutcome::Response)?;
    let deletion = ledger.begin_delete(&allocation)?;
    assert!(deletion.matches_session(&connection_id, 10, "session-a"));
    assert!(!deletion.matches_session(&connection_id, 11, "session-a"));
    Ok(())
}

#[test]
fn stale_poll_result_cannot_change_the_current_attempt_or_release_charge() -> TestResult {
    let mut ledger = HeaderLedger::new(2)?;
    let allocation = reserve(&mut ledger, connection("essential")?, 10, "session-a", 1)?;
    let first = ledger.begin_poll(&allocation)?;
    let late_copy = first.duplicate_for_test();
    ledger.finish_poll(&first, PollOutcome::Unknown)?;

    assert_eq!(
        ledger.finish_poll(&late_copy, PollOutcome::Response),
        Err(HeaderLedgerError::StaleAttempt)
    );
    let snapshot = ledger.snapshot();
    assert_eq!(snapshot.records[0].state, GenerationState::Uncertain);
    assert_eq!(ledger.charged_capacity(), 1);
    Ok(())
}

#[test]
fn restart_turns_an_in_flight_poll_into_sticky_uncertainty() -> TestResult {
    let mut ledger = HeaderLedger::new(2)?;
    let allocation = reserve(&mut ledger, connection("essential")?, 10, "session-a", 2)?;
    let _request = ledger.begin_poll(&allocation)?;

    let persisted = serde_json::to_vec(&ledger.snapshot())?;
    let snapshot = serde_json::from_slice(&persisted)?;
    let mut restored = HeaderLedger::restore(snapshot)?;
    assert_eq!(restored.charged_capacity(), 2);
    assert_eq!(
        restored.snapshot().records[0].state,
        GenerationState::Uncertain
    );
    assert_eq!(
        reserve(&mut restored, connection("essential")?, 10, "session-b", 1),
        Err(HeaderLedgerError::SessionStillCharged)
    );
    let restored_allocation = reserve(&mut restored, connection("essential")?, 10, "session-a", 2)?;
    assert_eq!(restored_allocation, allocation);
    assert_eq!(
        restored.begin_poll(&restored_allocation),
        Err(HeaderLedgerError::UnsettledPoll)
    );
    assert_eq!(
        restored.snapshot().records[0].state,
        GenerationState::Uncertain
    );
    assert_eq!(restored.charged_capacity(), 2);
    Ok(())
}

#[test]
fn timeout_or_restart_never_transfers_capacity_between_connections() -> TestResult {
    let mut ledger = HeaderLedger::new(3)?;
    let first = reserve(&mut ledger, connection("essential")?, 10, "session-a", 2)?;
    let poll = ledger.begin_poll(&first)?;
    ledger.finish_poll(&poll, PollOutcome::Unknown)?;
    let restored = HeaderLedger::restore(ledger.snapshot())?;

    assert_eq!(restored.charged_capacity(), 2);
    assert_eq!(
        restored
            .clone()
            .reserve(connection("chainargos")?, 20, "session-b", 2),
        Err(HeaderLedgerError::OverBudget)
    );
    Ok(())
}

#[test]
fn release_requires_settled_polls_zero_header_and_exact_delete() -> TestResult {
    let mut ledger = HeaderLedger::new(3)?;
    let allocation = reserve(&mut ledger, connection("essential")?, 10, "session-a", 2)?;
    let in_flight = ledger.begin_poll(&allocation)?;
    assert_eq!(
        ledger.begin_delete(&allocation),
        Err(HeaderLedgerError::PollStillInFlight)
    );
    ledger.finish_poll(&in_flight, PollOutcome::Response)?;
    assert_eq!(
        ledger.begin_delete(&allocation),
        Err(HeaderLedgerError::ZeroHeaderNotSettled)
    );
    let zero_request = ledger.begin_zero_poll(&allocation)?;
    assert_eq!(zero_request.header_value(), 0);
    ledger.finish_poll(&zero_request, PollOutcome::Response)?;
    let deletion = ledger.begin_delete(&allocation)?;
    ledger.finish_delete(&deletion)?;

    assert_eq!(ledger.charged_capacity(), 0);
    assert_eq!(
        reserve(&mut ledger, connection("essential")?, 10, "session-a", 2),
        Err(HeaderLedgerError::SessionRetired)
    );
    reserve(&mut ledger, connection("essential")?, 10, "session-b", 2)?;
    Ok(())
}
