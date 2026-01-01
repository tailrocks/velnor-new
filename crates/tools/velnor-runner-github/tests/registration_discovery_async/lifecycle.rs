use super::*;

#[test]
fn async_issue_stops_before_post_when_durable_intent_fails() {
    let mut transport = FakeTransport::default();
    transport
        .0
        .lock()
        .expect("test transport lock")
        .responses
        .push_back(response(200, private_admin_repo()));
    let evidence = block_on_ready(read_repository_admin_evidence_async(
        &mut transport,
        "ChainArgos",
        "java-monorepo",
        "hostcredential",
    ))
    .expect("private admin evidence");
    let mut intents = Intents::default();
    intents.0.lock().expect("test intent lock").fail_persist = true;

    let result = block_on_ready(issue_repository_discovery_token_async(
        &mut transport,
        evidence,
        "hostcredential",
        &mut intents,
    ));

    assert!(result.is_err());
    assert_eq!(
        transport
            .0
            .lock()
            .expect("test transport lock")
            .requests
            .len(),
        1
    );
}

#[test]
fn async_post_timeout_is_uncertain_and_is_not_retried() {
    let mut transport = FakeTransport::default();
    transport
        .0
        .lock()
        .expect("test transport lock")
        .responses
        .extend([
            response(200, private_admin_repo()),
            Err(TransportFail::Timeout),
        ]);
    let evidence = block_on_ready(read_repository_admin_evidence_async(
        &mut transport,
        "ChainArgos",
        "java-monorepo",
        "hostcredential",
    ))
    .expect("private admin evidence");
    let mut intents = Intents::default();

    let result = block_on_ready(issue_repository_discovery_token_async(
        &mut transport,
        evidence,
        "hostcredential",
        &mut intents,
    ));

    assert!(matches!(result, Err(SessionError::Uncertain)));
    assert_eq!(
        transport
            .0
            .lock()
            .expect("test transport lock")
            .requests
            .len(),
        2
    );
    assert_eq!(
        intents.0.lock().expect("test intent lock").rows[0].2,
        Some(DiscoveryCredentialOutcome::Uncertain)
    );
}

#[test]
fn malformed_registration_credential_is_uncertain_and_stops_before_admin_exchange() {
    let mut transport = FakeTransport::default();
    transport
        .0
        .lock()
        .expect("test transport lock")
        .responses
        .extend([
            response(200, private_admin_repo()),
            response(201, r#"{"token":"bad token"}"#),
        ]);
    let evidence = block_on_ready(read_repository_admin_evidence_async(
        &mut transport,
        "ChainArgos",
        "java-monorepo",
        "hostcredential",
    ))
    .expect("private admin evidence");
    let mut intents = Intents::default();

    let result = block_on_ready(issue_repository_discovery_token_async(
        &mut transport,
        evidence,
        "hostcredential",
        &mut intents,
    ));

    assert!(matches!(result, Err(SessionError::Uncertain)));
    assert_eq!(
        transport
            .0
            .lock()
            .expect("test transport lock")
            .requests
            .len(),
        2
    );
    assert_eq!(
        intents.0.lock().expect("test intent lock").rows[0].2,
        Some(DiscoveryCredentialOutcome::Uncertain)
    );
}

#[test]
fn cancelled_dispatched_post_signals_worker_and_keeps_pending_intent() {
    let mut transport = FakeTransport::default();
    transport
        .0
        .lock()
        .expect("test transport lock")
        .responses
        .push_back(response(200, private_admin_repo()));
    let evidence = block_on_ready(read_repository_admin_evidence_async(
        &mut transport,
        "ChainArgos",
        "java-monorepo",
        "hostcredential",
    ))
    .expect("private admin evidence");
    let mut intents = Intents::default();
    transport
        .0
        .lock()
        .expect("test transport lock")
        .pending_next = true;
    let mut operation = Box::pin(issue_repository_discovery_token_async(
        &mut transport,
        evidence,
        "hostcredential",
        &mut intents,
    ));

    assert!(matches!(poll_once(&mut operation), Poll::Pending));
    drop(operation);

    let state = intents.0.lock().expect("test intent lock").clone();
    assert_eq!(state.rows.len(), 1);
    assert_eq!(state.rows[0].2, None);
    let transport = transport.0.lock().expect("test transport lock");
    assert_eq!(transport.requests.len(), 2);
    assert!(
        transport
            .cancellations
            .last()
            .is_some_and(|cancel| cancel.load(Ordering::Acquire))
    );
}

#[test]
fn failed_outcome_write_discards_issued_secret_and_returns_uncertain() {
    let mut transport = FakeTransport::default();
    transport
        .0
        .lock()
        .expect("test transport lock")
        .responses
        .extend([
            response(200, private_admin_repo()),
            response(201, r#"{"token":"regtoken"}"#),
        ]);
    let evidence = block_on_ready(read_repository_admin_evidence_async(
        &mut transport,
        "ChainArgos",
        "java-monorepo",
        "hostcredential",
    ))
    .expect("private admin evidence");
    let mut intents = Intents::default();
    intents.0.lock().expect("test intent lock").fail_finish = true;

    let result = block_on_ready(issue_repository_discovery_token_async(
        &mut transport,
        evidence,
        "hostcredential",
        &mut intents,
    ));

    assert!(matches!(result, Err(SessionError::Uncertain)));
    assert_eq!(
        transport
            .0
            .lock()
            .expect("test transport lock")
            .requests
            .len(),
        2
    );
    assert_eq!(intents.0.lock().expect("test intent lock").rows[0].2, None);
}

#[test]
fn cancelled_exchange_does_not_issue_followup_admin_get() {
    let mut transport = FakeTransport::default();
    transport
        .0
        .lock()
        .expect("test transport lock")
        .responses
        .extend([
            response(200, private_admin_repo()),
            response(201, r#"{"token":"regtoken"}"#),
        ]);
    let evidence = block_on_ready(read_repository_admin_evidence_async(
        &mut transport,
        "ChainArgos",
        "java-monorepo",
        "hostcredential",
    ))
    .expect("private admin evidence");
    let mut intents = Intents::default();
    let token = block_on_ready(issue_repository_discovery_token_async(
        &mut transport,
        evidence,
        "hostcredential",
        &mut intents,
    ))
    .expect("registration token");
    transport
        .0
        .lock()
        .expect("test transport lock")
        .pending_next = true;
    let mut operation = Box::pin(exchange_repository_discovery_admin_once_async(
        &mut transport,
        token,
        &mut intents,
    ));

    assert!(matches!(poll_once(&mut operation), Poll::Pending));
    drop(operation);

    let transport = transport.0.lock().expect("test transport lock");
    assert_eq!(transport.requests.len(), 3);
    assert!(
        transport
            .cancellations
            .last()
            .is_some_and(|cancel| cancel.load(Ordering::Acquire))
    );
    assert_eq!(intents.0.lock().expect("test intent lock").rows[1].2, None);
}

#[test]
fn malformed_admin_credential_is_uncertain_and_stops_before_metadata_get() {
    let mut transport = FakeTransport::default();
    transport
        .0
        .lock()
        .expect("test transport lock")
        .responses
        .extend([
            response(200, private_admin_repo()),
            response(201, r#"{"token":"regtoken"}"#),
            response(
                200,
                r#"{"url":"https://pipelinesghubeus9.actions.githubusercontent.com/","token":"bad\ttoken"}"#,
            ),
        ]);
    let evidence = block_on_ready(read_repository_admin_evidence_async(
        &mut transport,
        "ChainArgos",
        "java-monorepo",
        "hostcredential",
    ))
    .expect("private admin evidence");
    let mut intents = Intents::default();
    let token = block_on_ready(issue_repository_discovery_token_async(
        &mut transport,
        evidence,
        "hostcredential",
        &mut intents,
    ))
    .expect("registration token");

    let result = block_on_ready(exchange_repository_discovery_admin_once_async(
        &mut transport,
        token,
        &mut intents,
    ));

    assert!(matches!(result, Err(SessionError::Uncertain)));
    assert_eq!(
        transport
            .0
            .lock()
            .expect("test transport lock")
            .requests
            .len(),
        3
    );
    assert_eq!(
        intents.0.lock().expect("test intent lock").rows[1].2,
        Some(DiscoveryCredentialOutcome::Uncertain)
    );
}
