use super::*;

#[tokio::test]
async fn duplicate_available_rows_trigger_only_one_admission_callback() -> Result<(), String> {
    let mut polled = available_poll();
    let Poll::Batch(batch) = &mut polled else {
        return Err("test poll is not a batch".to_owned());
    };
    batch.jobs.push(batch.jobs[0].clone());
    let calls = Cell::new(0);

    let result = admit_available(&polled, || async {
        calls.set(calls.get() + 1);
        Ok(PositiveAdmission::NoAcquire)
    })
    .await;
    assert!(result.is_ok());
    assert_eq!(calls.get(), 1);
    Ok(())
}

#[test]
fn acquired_ownership_survives_acknowledgement_and_settlement_uncertainty() {
    let token = Box::new(ReservationToken { request_id: 91 });
    let token_address = std::ptr::addr_of!(*token);
    let outcome = make_outcome(
        intake_decision(),
        Some(token),
        Acknowledgement::Uncertain,
        Settlement::Uncertain,
        &available_poll(),
    );

    let retained = outcome.admission.expect("acquired token is retained");
    assert_eq!(retained.request_id, 91);
    assert!(std::ptr::eq(token_address, &*retained));
    assert_eq!(outcome.acknowledgement, Acknowledgement::Uncertain);
    assert_eq!(outcome.settlement, Settlement::Uncertain);
    assert_eq!(outcome.message_id, Some(17));
    assert_eq!(
        outcome.cursor_disposition,
        CursorDisposition::Deferred { message_id: 17 }
    );
}

#[test]
fn settlement_uncertainty_keeps_an_acknowledged_acquired_token() {
    let token = Box::new(ReservationToken { request_id: 92 });
    let token_address = std::ptr::addr_of!(*token);
    let outcome = make_outcome(
        intake_decision(),
        Some(token),
        Acknowledgement::Acknowledged,
        Settlement::Uncertain,
        &available_poll(),
    );

    let retained = outcome.admission.expect("acquired token is retained");
    assert_eq!(retained.request_id, 92);
    assert!(std::ptr::eq(token_address, &*retained));
    assert_eq!(outcome.acknowledgement, Acknowledgement::Acknowledged);
    assert_eq!(outcome.settlement, Settlement::Uncertain);
    assert_eq!(outcome.cursor_disposition, CursorDisposition::ResetToHead);
}

#[test]
fn cursor_retries_known_unacknowledged_head_and_resets_after_ack() {
    assert_eq!(
        cursor_disposition(Some(19), Acknowledgement::Deferred, &available_poll()),
        CursorDisposition::Deferred { message_id: 19 }
    );
    assert_eq!(
        cursor_disposition(Some(19), Acknowledgement::Uncertain, &available_poll()),
        CursorDisposition::Deferred { message_id: 19 }
    );
    assert_eq!(
        cursor_disposition(Some(19), Acknowledgement::Acknowledged, &available_poll()),
        CursorDisposition::ResetToHead
    );
    assert_eq!(
        cursor_disposition(Some(0), Acknowledgement::Deferred, &available_poll()),
        CursorDisposition::Unchanged
    );
    assert_eq!(
        cursor_disposition(Some(0), Acknowledgement::Acknowledged, &available_poll()),
        CursorDisposition::ResetToHead
    );
    assert_eq!(
        cursor_disposition(None, Acknowledgement::NotRequired, &Poll::Empty),
        CursorDisposition::Unchanged
    );
}
