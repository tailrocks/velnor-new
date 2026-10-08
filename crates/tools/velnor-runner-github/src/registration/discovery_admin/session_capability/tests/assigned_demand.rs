use super::*;

#[test]
fn assigned_population_jit_is_distinct_from_acquire_and_ack_follows_jit() -> Result<(), &'static str>
{
    let jit_reply = Ok(exchange(200, r#"{"encodedJITConfig":"encoded-test"}"#));
    let second_jit_reply = Ok(exchange(200, r#"{"encodedJITConfig":"encoded-test-2"}"#));
    let ack_reply = Ok(exchange(204, ""));
    let close_reply = Ok(exchange(204, ""));
    let (mut script, mut capability, mut session, _) = capability_and_polled_session(
        ASSIGNED_EXCESS_DEMAND,
        [jit_reply, second_jit_reply, ack_reply, close_reply],
    )?;

    let mut demand = capability
        .take_assigned_demand(&mut session, 2)
        .map_err(|_| "take assigned")?
        .ok_or("two reserved assigned slots")?;
    run_two_assigned_jits(&mut script, &capability, &mut session, &mut demand)?;

    assert_eq!(
        capability
            .acknowledge_resolved_message(
                &mut script,
                &mut session,
                23,
                true,
                &RefreshGate::new(),
                |_, _, _| Ok(QUEUE_PATH.to_owned()),
            )
            .map_err(|_| "ack")?,
        Ack::Deleted
    );
    assert_eq!(script.seen.len(), 6);
    assert_eq!(script.seen[5].method, Method::Delete);
    assert_eq!(script.origins[5], "https://queue.example");
    assert_eq!(bearer(&script.seen[5]), Some("Bearer queue-canary"));
    assert_eq!(script.seen[5].path, format!("{QUEUE_PATH}/23"));

    assert_eq!(
        capability
            .close_session(&mut script, &mut session)
            .map_err(|_| "close")?,
        SessionCloseOutcome::Closed
    );
    assert_eq!(script.seen.len(), 7);
    assert_eq!(script.seen[6].method, Method::Delete);
    assert_eq!(
        script.origins[6],
        "https://pipelinesghubeus9.actions.githubusercontent.com"
    );
    assert!(
        script.seen[6]
            .path
            .ends_with("/runnerscalesets/7/sessions/session-1")
    );
    assert_eq!(bearer(&script.seen[6]), Some("Bearer admin-canary"));
    Ok(())
}

#[test]
fn available_offer_never_enters_assigned_population_jit() -> Result<(), &'static str> {
    let (script, capability, mut session, _) =
        capability_and_polled_session(AVAILABLE_WITH_DEMAND, std::iter::empty())?;
    assert!(
        capability
            .take_assigned_demand(&mut session, 1)
            .map_err(|_| "take")?
            .is_none()
    );
    assert_eq!(script.seen.len(), 3);
    assert!(
        script
            .seen
            .iter()
            .all(|request| !request.path.contains("generatejitconfig"))
    );
    Ok(())
}

#[test]
fn zero_deficit_or_capacity_creates_no_assigned_permit() -> Result<(), &'static str> {
    let (script, capability, mut session, _) =
        capability_and_polled_session(STARTED_WITH_NO_DEMAND, std::iter::empty())?;
    assert!(
        capability
            .take_assigned_demand(&mut session, 1)
            .map_err(|_| "zero deficit")?
            .is_none()
    );
    assert!(
        capability
            .take_assigned_demand(&mut session, 0)
            .map_err(|_| "zero capacity")?
            .is_none()
    );
    assert_eq!(script.seen.len(), 3);
    Ok(())
}

#[test]
fn uncertain_assigned_jit_blocks_retry_ack_poll_and_close() -> Result<(), &'static str> {
    let (mut script, mut capability, mut session, _) =
        capability_and_polled_session(ASSIGNED_WITH_DEMAND, [Err(TransportFail::Reset)])?;
    let mut demand = capability
        .take_assigned_demand(&mut session, 1)
        .map_err(|_| "take")?
        .ok_or("demand")?;
    assert!(matches!(
        capability.jit_assigned_demand(&mut script, &mut session, &mut demand, "runner-test"),
        Err(crate::SessionError::Uncertain)
    ));
    assert_eq!(script.seen.len(), 4);
    assert!(matches!(
        capability.jit_assigned_demand(&mut script, &mut session, &mut demand, "runner-test"),
        Err(crate::SessionError::Wire(WireError::RegistrationRejected))
    ));
    assert!(
        capability
            .acknowledge_resolved_message(
                &mut script,
                &mut session,
                19,
                true,
                &RefreshGate::new(),
                |_, _, _| Ok(QUEUE_PATH.to_owned()),
            )
            .is_err()
    );
    assert_eq!(
        capability.close_session(&mut script, &mut session),
        Ok(SessionCloseOutcome::Held)
    );
    assert_eq!(script.seen.len(), 4);
    Ok(())
}
