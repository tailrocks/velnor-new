use super::*;

#[test]
fn zero_header_poll_can_repeat_while_the_same_session_drains() -> TestResult {
    let mut ledger = HeaderLedger::new(2)?;
    let allocation = reserve(&mut ledger, connection("essential")?, 10, "session-a", 2)?;
    let advertised = ledger.begin_poll(&allocation)?;
    ledger.finish_poll(&advertised, PollOutcome::Response)?;

    let first_zero = ledger.begin_zero_poll(&allocation)?;
    ledger.finish_poll(&first_zero, PollOutcome::Response)?;
    assert_eq!(ledger.charged_capacity(), 2);
    assert_eq!(
        ledger.begin_poll(&allocation),
        Err(HeaderLedgerError::ZeroHeaderSettled)
    );

    let second_zero = ledger.begin_zero_poll(&allocation)?;
    assert_eq!(second_zero.header_value(), 0);
    assert_eq!(ledger.charged_capacity(), 2);
    ledger.finish_poll(&second_zero, PollOutcome::Response)?;

    let deletion = ledger.begin_delete(&allocation)?;
    ledger.finish_delete(&deletion)?;
    assert_eq!(ledger.charged_capacity(), 0);
    Ok(())
}

#[test]
fn unknown_repeated_zero_poll_stays_uncertain_and_charged_after_restart() -> TestResult {
    let mut ledger = HeaderLedger::new(2)?;
    let allocation = reserve(&mut ledger, connection("essential")?, 10, "session-a", 2)?;
    let advertised = ledger.begin_poll(&allocation)?;
    ledger.finish_poll(&advertised, PollOutcome::Response)?;
    let first_zero = ledger.begin_zero_poll(&allocation)?;
    ledger.finish_poll(&first_zero, PollOutcome::Response)?;
    let second_zero = ledger.begin_zero_poll(&allocation)?;
    ledger.finish_poll(&second_zero, PollOutcome::Unknown)?;

    let mut reopened = HeaderLedger::restore(ledger.snapshot())?;
    assert_eq!(reopened.charged_capacity(), 2);
    assert_eq!(
        reopened.begin_zero_poll(&allocation),
        Err(HeaderLedgerError::UnsettledPoll)
    );
    assert_eq!(
        reopened.begin_delete(&allocation),
        Err(HeaderLedgerError::UnsettledPoll)
    );
    assert_eq!(
        reserve(&mut reopened, connection("essential")?, 10, "session-b", 1),
        Err(HeaderLedgerError::SessionStillCharged)
    );
    Ok(())
}

#[test]
fn restart_after_zero_poll_keeps_charge_until_exact_session_delete() -> TestResult {
    let mut ledger = HeaderLedger::new(2)?;
    let allocation = reserve(&mut ledger, connection("essential")?, 10, "session-a", 2)?;
    let poll = ledger.begin_poll(&allocation)?;
    ledger.finish_poll(&poll, PollOutcome::Response)?;
    let zero = ledger.begin_zero_poll(&allocation)?;
    ledger.finish_poll(&zero, PollOutcome::Response)?;
    let deletion = ledger.begin_delete(&allocation)?;
    let mut restored = HeaderLedger::restore(ledger.snapshot())?;

    assert_eq!(restored.charged_capacity(), 2);
    assert_eq!(
        restored.begin_poll(&allocation),
        Err(HeaderLedgerError::DeleteOutcomeUncertain)
    );
    assert_eq!(
        restored.begin_delete(&allocation),
        Err(HeaderLedgerError::DeleteOutcomeUncertain)
    );
    assert_eq!(
        restored.finish_delete(&deletion),
        Err(HeaderLedgerError::StaleAttempt)
    );
    assert_eq!(restored.charged_capacity(), 2);
    Ok(())
}

#[test]
fn unknown_remote_poll_cannot_be_released_by_later_delete_proofs() -> TestResult {
    let mut ledger = HeaderLedger::new(2)?;
    let allocation = reserve(&mut ledger, connection("essential")?, 10, "session-a", 1)?;
    let request = ledger.begin_poll(&allocation)?;
    ledger.finish_poll(&request, PollOutcome::Unknown)?;
    assert_eq!(
        ledger.begin_delete(&allocation),
        Err(HeaderLedgerError::UnsettledPoll)
    );
    assert_eq!(
        ledger.begin_poll(&allocation),
        Err(HeaderLedgerError::UnsettledPoll)
    );
    assert_eq!(
        ledger.begin_zero_poll(&allocation),
        Err(HeaderLedgerError::UnsettledPoll)
    );
    assert_eq!(ledger.charged_capacity(), 1);
    Ok(())
}

#[test]
fn late_delete_response_after_unknown_outcome_cannot_release_charge() -> TestResult {
    let mut ledger = HeaderLedger::new(3)?;
    let first = reserve(&mut ledger, connection("essential")?, 10, "session-a", 1)?;
    let zero = ledger.begin_zero_poll(&first)?;
    ledger.finish_poll(&zero, PollOutcome::Response)?;
    let delete = ledger.begin_delete(&first)?;
    let late_response = delete.duplicate_for_test();
    ledger.mark_delete_unknown(&delete)?;

    assert_eq!(
        ledger.finish_delete(&late_response),
        Err(HeaderLedgerError::StaleAttempt)
    );
    assert_eq!(ledger.charged_capacity(), 1);
    Ok(())
}

#[test]
fn interrupted_delete_is_sticky_across_restart() -> TestResult {
    let mut ledger = HeaderLedger::new(2)?;
    let allocation = reserve(&mut ledger, connection("essential")?, 10, "session-a", 2)?;
    let zero = ledger.begin_zero_poll(&allocation)?;
    ledger.finish_poll(&zero, PollOutcome::Response)?;
    let _delete = ledger.begin_delete(&allocation)?;

    let mut restored = HeaderLedger::restore(ledger.snapshot())?;
    assert_eq!(restored.charged_capacity(), 2);
    assert_eq!(
        restored.begin_delete(&allocation),
        Err(HeaderLedgerError::DeleteOutcomeUncertain)
    );
    assert_eq!(
        reserve(&mut restored, connection("chainargos")?, 20, "session-b", 1),
        Err(HeaderLedgerError::OverBudget)
    );
    Ok(())
}

#[test]
fn snapshot_validation_rejects_over_budget_or_duplicate_live_scale_sets() -> TestResult {
    let mut ledger = HeaderLedger::new(3)?;
    reserve(&mut ledger, connection("essential")?, 10, "session-a", 2)?;
    let mut snapshot = ledger.snapshot();
    snapshot.records[0].target = 4;

    assert_eq!(
        HeaderLedger::restore(snapshot),
        Err(HeaderLedgerError::InvalidSnapshot)
    );

    let mut ledger = HeaderLedger::new(3)?;
    reserve(&mut ledger, connection("essential")?, 10, "session-a", 1)?;
    let mut snapshot = ledger.snapshot();
    snapshot.records[0].zero_header_settled = true;
    assert_eq!(
        HeaderLedger::restore(snapshot),
        Err(HeaderLedgerError::InvalidSnapshot)
    );

    let mut ledger = HeaderLedger::new(3)?;
    reserve(&mut ledger, connection("essential")?, 10, "session-a", 1)?;
    let mut snapshot = ledger.snapshot();
    let mut duplicate = snapshot.records[0].clone();
    duplicate.identity.session_id = SessionId("session-b".to_owned());
    duplicate.generation = HeaderGeneration(2);
    duplicate.attempt_sequence = 0;
    duplicate.in_flight_poll = None;
    duplicate.zero_header_settled = false;
    snapshot.next_generation = 3;
    snapshot.records.push(duplicate);

    assert_eq!(
        HeaderLedger::restore(snapshot),
        Err(HeaderLedgerError::InvalidSnapshot)
    );
    Ok(())
}

#[test]
fn invalid_session_and_scale_set_inputs_fail_closed() -> TestResult {
    let mut ledger = HeaderLedger::new(2)?;
    assert_eq!(
        reserve(&mut ledger, connection("essential")?, 0, "session-a", 1),
        Err(HeaderLedgerError::InvalidScaleSetId)
    );
    assert_eq!(
        reserve(&mut ledger, connection("essential")?, 10, "", 1),
        Err(HeaderLedgerError::InvalidSessionId)
    );
    assert_eq!(
        reserve(&mut ledger, connection("essential")?, 10, "bad\nsession", 1),
        Err(HeaderLedgerError::InvalidSessionId)
    );
    assert_eq!(
        reserve(
            &mut ledger,
            connection("essential")?,
            10,
            "session-a",
            i32::MAX as u32 + 1
        ),
        Err(HeaderLedgerError::HeaderOutOfRange)
    );
    Ok(())
}
