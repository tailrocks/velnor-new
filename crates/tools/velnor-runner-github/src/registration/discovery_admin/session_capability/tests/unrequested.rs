use super::*;
use crate::AcquireUnresolvedReason;

#[test]
fn selected_offer_can_finish_while_other_verified_offer_is_explicitly_left_unrequested()
-> Result<(), &'static str> {
    let later = [
        Ok(exchange(200, TRUST_RUN_BODY)),
        Ok(exchange(200, TRUST_RUN_BODY)),
        Ok(exchange(200, TRUST_RUN_BODY)),
        Ok(exchange(200, r#"{"count":1,"value":[23]}"#)),
        Ok(exchange(200, r#"{"encodedJITConfig":"encoded-trusted"}"#)),
        Ok(exchange(204, "")),
    ];
    let (mut script, capability, mut session, batch) =
        capability_and_polled_session(MULTI_AVAILABLE_TRUSTED, later)?;

    let unrequested = verify_test_offer_at_index(&mut script, &batch, 1)?;
    assert_eq!(unrequested.runner_request_id(), 24);
    capability
        .leave_unrequested_available(&mut session, &unrequested)
        .map_err(|_| "leave unrequested")?;
    assert!(session.population_observation().is_none());

    // The same event cannot be reclassified twice; the first disposition is
    // one-shot and remains attached to this exact message.
    let duplicate = verify_test_offer_at_index(&mut script, &batch, 1)?;
    assert!(
        capability
            .leave_unrequested_available(&mut session, &duplicate)
            .is_err()
    );

    let selected = verify_test_offer_at_index(&mut script, &batch, 0)?;
    assert_eq!(selected.runner_request_id(), 23);
    let acquired = capability
        .acquire_verified(&mut script, &mut session, selected, &RefreshGate::new())
        .map_err(|_| "acquire selected offer")?;
    let VerifiedAcquireOutcome::Acquired(acquired) = acquired else {
        return Err("selected request acquired");
    };
    capability
        .jit_verified(&mut script, &mut session, *acquired, "runner-selected")
        .map_err(|_| "selected jit")?;
    assert_eq!(
        capability
            .acknowledge_resolved_message(&mut script, &mut session, 24, true, &RefreshGate::new(),)
            .map_err(|_| "ack message")?,
        Ack::Deleted
    );

    let acquire_requests: Vec<_> = script
        .seen
        .iter()
        .filter(|request| request.path.ends_with("/acquirejobs"))
        .collect();
    assert_eq!(acquire_requests.len(), 1);
    assert_eq!(acquire_requests[0].body, b"[23]");
    assert_eq!(
        script.seen.last().map(|request| request.method),
        Some(Method::Delete)
    );
    Ok(())
}

#[test]
fn submitted_but_omitted_offer_cannot_be_reclassified_as_unrequested() -> Result<(), &'static str> {
    let later = [
        Ok(exchange(200, TRUST_RUN_BODY)),
        Ok(exchange(200, r#"{"count":0,"value":[]}"#)),
        Ok(exchange(200, TRUST_RUN_BODY)),
    ];
    let (mut script, capability, mut session, batch) =
        capability_and_polled_session(AVAILABLE_TRUSTED, later)?;
    let offered = verify_test_offer(&mut script, &batch)?;
    let outcome = capability
        .acquire_verified(&mut script, &mut session, offered, &RefreshGate::new())
        .map_err(|_| "acquire response")?;
    assert!(matches!(
        outcome,
        VerifiedAcquireOutcome::Unresolved {
            reason: AcquireUnresolvedReason::Omitted,
            ..
        }
    ));

    let reclassified = verify_test_offer(&mut script, &batch)?;
    assert!(
        capability
            .leave_unrequested_available(&mut session, &reclassified)
            .is_err()
    );
    assert_eq!(
        capability
            .acknowledge_resolved_message(&mut script, &mut session, 22, true, &RefreshGate::new(),)
            .map_err(|_| "held ack")?,
        Ack::Suppressed
    );
    assert!(
        script
            .seen
            .iter()
            .all(|request| request.method != Method::Delete)
    );
    Ok(())
}

#[test]
fn uncertain_acquire_cannot_be_reclassified_as_unrequested() -> Result<(), &'static str> {
    let later = [
        Ok(exchange(200, TRUST_RUN_BODY)),
        Err(TransportFail::Reset),
        Ok(exchange(200, TRUST_RUN_BODY)),
    ];
    let (mut script, capability, mut session, batch) =
        capability_and_polled_session(AVAILABLE_TRUSTED, later)?;
    let offered = verify_test_offer(&mut script, &batch)?;
    assert!(
        capability
            .acquire_verified(&mut script, &mut session, offered, &RefreshGate::new())
            .is_err()
    );

    let reclassified = verify_test_offer(&mut script, &batch)?;
    assert!(
        capability
            .leave_unrequested_available(&mut session, &reclassified)
            .is_err()
    );
    assert_eq!(
        capability
            .acknowledge_resolved_message(&mut script, &mut session, 22, true, &RefreshGate::new(),)
            .map_err(|_| "held ack")?,
        Ack::Suppressed
    );
    assert!(
        script
            .seen
            .iter()
            .all(|request| request.method != Method::Delete)
    );
    Ok(())
}
