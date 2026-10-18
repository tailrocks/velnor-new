use super::{
    GenerationState, HeaderAllocation, HeaderGeneration, HeaderLedger, HeaderLedgerError,
    PollOutcome, PollsSettledProof, SessionDeleteProof, SessionId,
};
use crate::config::ConnectionId;

fn connection(value: &str) -> ConnectionId {
    ConnectionId::parse(value).expect("test connection ID is valid")
}

fn reserve(
    ledger: &mut HeaderLedger,
    connection_id: &str,
    scale_set_id: i64,
    session_id: &str,
    target: u32,
) -> Result<HeaderAllocation, HeaderLedgerError> {
    ledger.reserve(connection(connection_id), scale_set_id, session_id, target)
}

#[test]
fn exact_reservation_is_idempotent_and_zero_is_a_valid_header() {
    let mut ledger = HeaderLedger::new(3).expect("positive host capacity");
    let zero =
        reserve(&mut ledger, "essential", 10, "session-a", 0).expect("zero capacity is accepted");
    let repeated = reserve(&mut ledger, "essential", 10, "session-a", 0)
        .expect("same identity and target reuse the allocation");

    assert_eq!(zero, repeated);
    assert_eq!(ledger.charged_capacity(), 0);
}

#[test]
fn aggregate_capacity_is_reserved_before_a_second_connection_can_poll() {
    let mut ledger = HeaderLedger::new(3).expect("positive host capacity");
    reserve(&mut ledger, "essential", 10, "session-a", 2).expect("first target fits");
    reserve(&mut ledger, "chainargos", 20, "session-b", 1).expect("aggregate fits");

    assert_eq!(ledger.charged_capacity(), 3);
    assert_eq!(
        reserve(&mut ledger, "third", 30, "session-c", 1),
        Err(HeaderLedgerError::OverBudget)
    );
    assert_eq!(ledger.charged_capacity(), 3);
}

#[test]
fn retry_reuses_charge_but_uncertainty_blocks_target_or_session_replacement() {
    let mut ledger = HeaderLedger::new(4).expect("positive host capacity");
    let allocation =
        reserve(&mut ledger, "essential", 10, "session-a", 2).expect("reservation succeeds");
    let request = ledger.begin_poll(&allocation).expect("poll intent starts");
    ledger
        .finish_poll(request, PollOutcome::Unknown)
        .expect("transport outcome is uncertain");

    let retry = reserve(&mut ledger, "essential", 10, "session-a", 2)
        .expect("same exact session and target reuse its charge");
    assert_eq!(retry, allocation);
    assert_eq!(ledger.charged_capacity(), 2);
    assert_eq!(
        reserve(&mut ledger, "essential", 10, "session-a", 1),
        Err(HeaderLedgerError::TargetChanged)
    );
    assert_eq!(
        reserve(&mut ledger, "essential", 10, "session-b", 2),
        Err(HeaderLedgerError::SessionStillCharged)
    );
    assert_eq!(ledger.charged_capacity(), 2);
}

#[test]
fn only_one_poll_is_in_flight_per_exact_session_generation() {
    let mut ledger = HeaderLedger::new(2).expect("positive host capacity");
    let allocation =
        reserve(&mut ledger, "essential", 10, "session-a", 1).expect("reservation succeeds");
    let first = ledger.begin_poll(&allocation).expect("first poll starts");

    assert_eq!(
        ledger.begin_poll(&allocation),
        Err(HeaderLedgerError::PollAlreadyInFlight)
    );
    ledger
        .finish_poll(first, PollOutcome::Response)
        .expect("completed request is recorded");
    assert_eq!(ledger.charged_capacity(), 1);
}

#[test]
fn stale_poll_result_cannot_change_the_current_attempt_or_release_charge() {
    let mut ledger = HeaderLedger::new(2).expect("positive host capacity");
    let allocation =
        reserve(&mut ledger, "essential", 10, "session-a", 1).expect("reservation succeeds");
    let first = ledger.begin_poll(&allocation).expect("first poll starts");
    let late_copy = first.clone();
    ledger
        .finish_poll(first, PollOutcome::Unknown)
        .expect("unknown outcome is sticky");

    assert_eq!(
        ledger.finish_poll(late_copy, PollOutcome::Response),
        Err(HeaderLedgerError::StaleAttempt)
    );
    let snapshot = ledger.snapshot();
    assert_eq!(snapshot.records[0].state, GenerationState::Uncertain);
    assert_eq!(ledger.charged_capacity(), 1);
}

#[test]
fn restart_turns_an_in_flight_poll_into_sticky_uncertainty() {
    let mut ledger = HeaderLedger::new(2).expect("positive host capacity");
    let allocation =
        reserve(&mut ledger, "essential", 10, "session-a", 2).expect("reservation succeeds");
    let _request = ledger.begin_poll(&allocation).expect("poll intent starts");

    let persisted = serde_json::to_vec(&ledger.snapshot()).expect("snapshot serializes");
    let snapshot = serde_json::from_slice(&persisted).expect("snapshot deserializes");
    let mut restored = HeaderLedger::restore(snapshot).expect("snapshot is valid");
    assert_eq!(restored.charged_capacity(), 2);
    assert_eq!(
        restored.snapshot().records[0].state,
        GenerationState::Uncertain
    );
    assert_eq!(
        reserve(&mut restored, "essential", 10, "session-b", 1),
        Err(HeaderLedgerError::SessionStillCharged)
    );
    let restored_allocation = reserve(&mut restored, "essential", 10, "session-a", 2)
        .expect("exact session and target resume without double charge");
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
}

#[test]
fn timeout_or_restart_never_transfers_capacity_between_connections() {
    let mut ledger = HeaderLedger::new(3).expect("positive host capacity");
    let first =
        reserve(&mut ledger, "essential", 10, "session-a", 2).expect("first allocation succeeds");
    let poll = ledger.begin_poll(&first).expect("poll starts");
    ledger
        .finish_poll(poll, PollOutcome::Unknown)
        .expect("poll outcome remains uncertain");
    let restored = HeaderLedger::restore(ledger.snapshot()).expect("restart state is valid");

    assert_eq!(restored.charged_capacity(), 2);
    assert_eq!(
        restored
            .clone()
            .reserve(connection("chainargos"), 20, "session-b", 2),
        Err(HeaderLedgerError::OverBudget)
    );
}

#[test]
fn release_requires_settled_polls_zero_header_and_exact_delete() {
    let mut ledger = HeaderLedger::new(3).expect("positive host capacity");
    let allocation =
        reserve(&mut ledger, "essential", 10, "session-a", 2).expect("reservation succeeds");
    let in_flight = ledger.begin_poll(&allocation).expect("poll starts");
    let polls_settled = PollsSettledProof::test_only(&allocation);
    let deletion = SessionDeleteProof::test_only(&allocation);
    assert_eq!(
        ledger.release(&polls_settled, &deletion),
        Err(HeaderLedgerError::PollStillInFlight)
    );
    ledger
        .finish_poll(in_flight, PollOutcome::Response)
        .expect("owned request has returned");
    assert_eq!(
        ledger.release(&polls_settled, &deletion),
        Err(HeaderLedgerError::ZeroHeaderNotSettled)
    );
    let zero_request = ledger
        .begin_zero_poll(&allocation)
        .expect("retirement starts with a zero-header poll");
    assert_eq!(zero_request.header_value(), 0);
    ledger
        .finish_poll(zero_request, PollOutcome::Response)
        .expect("same-session zero-header request is durably settled");
    ledger
        .release(&polls_settled, &deletion)
        .expect("settled polls and exact delete proof retire the generation");

    assert_eq!(ledger.charged_capacity(), 0);
    assert_eq!(
        reserve(&mut ledger, "essential", 10, "session-a", 2),
        Err(HeaderLedgerError::SessionRetired)
    );
    reserve(&mut ledger, "essential", 10, "session-b", 2)
        .expect("a distinct session can start after release");
}

#[test]
fn restart_after_zero_poll_keeps_charge_until_exact_session_delete() {
    let mut ledger = HeaderLedger::new(2).expect("positive host capacity");
    let allocation =
        reserve(&mut ledger, "essential", 10, "session-a", 2).expect("reservation succeeds");
    let poll = ledger.begin_poll(&allocation).expect("initial poll starts");
    ledger
        .finish_poll(poll, PollOutcome::Response)
        .expect("initial poll is settled");
    let zero = ledger
        .begin_zero_poll(&allocation)
        .expect("zero-header request starts");
    ledger
        .finish_poll(zero, PollOutcome::Response)
        .expect("zero-header request completes");
    let mut restored =
        HeaderLedger::restore(ledger.snapshot()).expect("settled zero state survives restart");

    assert_eq!(restored.charged_capacity(), 2);
    assert_eq!(
        restored.begin_poll(&allocation),
        Err(HeaderLedgerError::ZeroHeaderSettled)
    );
    let polls_settled = PollsSettledProof::test_only(&allocation);
    let deletion = SessionDeleteProof::test_only(&allocation);
    restored
        .release(&polls_settled, &deletion)
        .expect("exact session deletion retires the old generation");
    assert_eq!(restored.charged_capacity(), 0);
}

#[test]
fn unknown_remote_poll_cannot_be_released_by_later_delete_proofs() {
    let mut ledger = HeaderLedger::new(2).expect("positive host capacity");
    let allocation =
        reserve(&mut ledger, "essential", 10, "session-a", 1).expect("reservation succeeds");
    let request = ledger.begin_poll(&allocation).expect("poll starts");
    ledger
        .finish_poll(request, PollOutcome::Unknown)
        .expect("remote outcome stays uncertain");
    let polls_settled = PollsSettledProof::test_only(&allocation);
    let deletion = SessionDeleteProof::test_only(&allocation);

    assert_eq!(
        ledger.release(&polls_settled, &deletion),
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
}

#[test]
fn release_rejects_session_and_generation_mismatch() {
    let mut ledger = HeaderLedger::new(3).expect("positive host capacity");
    let first =
        reserve(&mut ledger, "essential", 10, "session-a", 1).expect("first reservation succeeds");
    let other =
        reserve(&mut ledger, "chainargos", 20, "session-b", 1).expect("second connection fits");
    let polls_settled = PollsSettledProof::test_only(&first);
    let wrong_deletion = SessionDeleteProof::test_only(&other);

    assert_eq!(
        ledger.release(&polls_settled, &wrong_deletion),
        Err(HeaderLedgerError::ReleaseEvidenceMismatch)
    );
    assert_eq!(ledger.charged_capacity(), 2);
}

#[test]
fn snapshot_validation_rejects_over_budget_or_duplicate_live_scale_sets() {
    let mut ledger = HeaderLedger::new(3).expect("positive host capacity");
    reserve(&mut ledger, "essential", 10, "session-a", 2).expect("first allocation fits");
    let mut snapshot = ledger.snapshot();
    snapshot.records[0].target = 4;

    assert_eq!(
        HeaderLedger::restore(snapshot),
        Err(HeaderLedgerError::InvalidSnapshot)
    );

    let mut ledger = HeaderLedger::new(3).expect("positive host capacity");
    reserve(&mut ledger, "essential", 10, "session-a", 1).expect("allocation fits");
    let mut snapshot = ledger.snapshot();
    snapshot.records[0].zero_header_settled = true;
    assert_eq!(
        HeaderLedger::restore(snapshot),
        Err(HeaderLedgerError::InvalidSnapshot)
    );

    let mut ledger = HeaderLedger::new(3).expect("positive host capacity");
    reserve(&mut ledger, "essential", 10, "session-a", 1).expect("first allocation fits");
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
}

#[test]
fn invalid_session_and_scale_set_inputs_fail_closed() {
    let mut ledger = HeaderLedger::new(2).expect("positive host capacity");
    assert_eq!(
        reserve(&mut ledger, "essential", 0, "session-a", 1),
        Err(HeaderLedgerError::InvalidScaleSetId)
    );
    assert_eq!(
        reserve(&mut ledger, "essential", 10, "", 1),
        Err(HeaderLedgerError::InvalidSessionId)
    );
    assert_eq!(
        reserve(&mut ledger, "essential", 10, "bad\nsession", 1),
        Err(HeaderLedgerError::InvalidSessionId)
    );
    assert_eq!(
        reserve(
            &mut ledger,
            "essential",
            10,
            "session-a",
            i32::MAX as u32 + 1
        ),
        Err(HeaderLedgerError::HeaderOutOfRange)
    );
}
