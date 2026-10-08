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
            .acknowledge_resolved_message(&mut script, &mut session, 21, true, &RefreshGate::new(),)
            .map_err(|_| "ack")?,
        Ack::Deleted
    );
    let count = script.seen.len();
    let mut foreign_repository = TestCloseClaim::for_session(&session);
    foreign_repository.repository_id += 1;
    assert!(
        capability
            .close_session_claimed(&mut script, &mut session, &mut foreign_repository)
            .is_err()
    );
    let mut foreign_session = TestCloseClaim::for_session(&session);
    foreign_session.session_id = "another-session".to_owned();
    assert!(
        capability
            .close_session_claimed(&mut script, &mut session, &mut foreign_session)
            .is_err()
    );
    assert_eq!(script.seen.len(), count);
    assert!(!foreign_repository.attempted);
    assert!(!foreign_session.attempted);
    let mut claim = TestCloseClaim::for_session(&session);
    assert_eq!(
        capability.close_session_claimed(&mut script, &mut session, &mut claim),
        Err(crate::SessionError::Uncertain)
    );
    assert_eq!(script.seen.len(), count + 1);
    assert_eq!(
        script.origins[count],
        "https://pipelinesghubeus9.actions.githubusercontent.com"
    );
    assert!(
        capability
            .close_session_claimed(&mut script, &mut session, &mut claim)
            .is_err()
    );
    assert_eq!(script.seen.len(), count + 1);
    Ok(())
}
