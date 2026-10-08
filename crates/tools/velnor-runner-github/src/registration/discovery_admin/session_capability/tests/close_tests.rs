use super::*;

#[test]
fn clean_close_is_one_shot_and_uncertain_close_is_not_replayed() -> Result<(), &'static str> {
    let (mut script, mut capability, mut session, _) = capability_and_polled_session(
        STARTED_WITH_NO_DEMAND,
        [Ok(exchange(204, "")), Err(TransportFail::Reset)],
    )?;
    // Resolve the harmless lifecycle batch first so the exact session is closable.
    assert_eq!(
        capability
            .acknowledge_resolved_message(
                &mut script,
                &mut session,
                21,
                true,
                &RefreshGate::new(),
                |_, _, _| Ok(QUEUE_PATH.to_owned()),
            )
            .map_err(|_| "ack")?,
        Ack::Deleted
    );
    let count = script.seen.len();
    assert_eq!(
        capability.close_session(&mut script, &mut session),
        Err(crate::SessionError::Uncertain)
    );
    assert_eq!(script.seen.len(), count + 1);
    assert_eq!(
        script.origins[count],
        "https://pipelinesghubeus9.actions.githubusercontent.com"
    );
    assert!(capability.close_session(&mut script, &mut session).is_err());
    assert_eq!(script.seen.len(), count + 1);
    Ok(())
}
