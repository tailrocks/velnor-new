use super::*;

pub(in crate::launch::steps::scoped::runner::tests::docker_pair) async fn replay_owned_terminal_offer(
    fixture: Fixture,
    poll_session: PollSession,
    witness_request_id: i64,
    actual_request_id: i64,
) -> Result<(), String> {
    let fixture = fixture.reopen().await?;
    let mut response = SessionResponse;
    let mut session =
        velnor_runner_github::create_session(&mut response, SET_ID, "owner", "admin-token")
            .map_err(|error| error.to_string())?;
    if session.session_id != "native-intake-session" {
        return Err("The replay session does not match the saved session.".to_owned());
    }
    let mut link = crate::listen::inert_link_for_offline_test();
    let acquire_calls_before = poll_session.acquire_calls.get();
    let admission_calls = std::cell::Cell::new(0);
    let acknowledgement_calls = std::cell::Cell::new(0);
    assert_terminal_replay(
        &mut link,
        &mut session,
        &fixture.scope,
        &poll_session,
        &admission_calls,
        &acknowledgement_calls,
        actual_request_id,
        acquire_calls_before,
    )
    .await?;
    assert_acquired_only_defers(
        &mut link,
        &mut session,
        &fixture.scope,
        &poll_session,
        &admission_calls,
        &acknowledgement_calls,
        witness_request_id,
        acquire_calls_before,
    )
    .await?;
    Ok(())
}

async fn assert_terminal_replay(
    link: &mut crate::listen::Link,
    session: &mut velnor_runner_github::QueueSession,
    scoped: &crate::journal::ConnectionJournal,
    poll_session: &PollSession,
    admission_calls: &std::cell::Cell<usize>,
    acknowledgement_calls: &std::cell::Cell<usize>,
    actual_request_id: i64,
    acquire_calls_before: usize,
) -> Result<(), String> {
    let terminal_poll = available_poll(130, actual_request_id)?;
    let outcome = poll_once_with_test_effects(
        link,
        session,
        scoped,
        poll_session,
        terminal_poll.clone(),
        true,
        admission_calls,
        acknowledgement_calls,
    )
    .await
    .map_err(|error| error.to_string())?;
    assert_eq!(admission_calls.get(), 0);
    assert_eq!(acknowledgement_calls.get(), 1);
    let Some(crate::launch::PositiveAdmission::OwnedTerminal(receipt)) = outcome.admission.as_ref()
    else {
        return Err("The terminal replay receipt is absent.".to_owned());
    };
    assert!(receipt.matches_poll(&terminal_poll, scoped, SET_ID, &session.session_id));
    assert!(!receipt.matches_poll(&terminal_poll, scoped, SET_ID, "changed-session"));
    assert_job_id_change_is_rejected(&terminal_poll, scoped, session).await?;
    assert_source_change_is_rejected(&terminal_poll, scoped, session).await?;
    assert!(matches!(
        outcome.admission,
        Some(crate::launch::PositiveAdmission::OwnedTerminal(_))
    ));
    assert_eq!(
        outcome.acknowledgement,
        crate::launch::Acknowledgement::Acknowledged
    );
    assert_eq!(
        outcome.cursor_disposition,
        crate::launch::CursorDisposition::ResetToHead
    );
    assert_eq!(outcome.settlement, crate::launch::Settlement::Settled);
    assert_eq!(poll_session.acquire_calls.get(), acquire_calls_before);
    Ok(())
}

async fn assert_job_id_change_is_rejected(
    terminal_poll: &velnor_runner_github::Poll,
    scoped: &crate::journal::ConnectionJournal,
    session: &velnor_runner_github::QueueSession,
) -> Result<(), String> {
    let mut changed_job = terminal_poll.clone();
    if let velnor_runner_github::Poll::Batch(batch) = &mut changed_job {
        batch.jobs[0].job_id = Some("different-job".to_owned());
    }
    assert!(
        crate::launch::classify_owned_terminal_available(
            scoped,
            SET_ID,
            &session.session_id,
            &changed_job,
        )
        .await
        .map_err(|error| error.to_string())?
        .is_none()
    );
    Ok(())
}

async fn assert_source_change_is_rejected(
    terminal_poll: &velnor_runner_github::Poll,
    scoped: &crate::journal::ConnectionJournal,
    session: &velnor_runner_github::QueueSession,
) -> Result<(), String> {
    let mut changed_source = terminal_poll.clone();
    if let velnor_runner_github::Poll::Batch(batch) = &mut changed_source {
        batch.jobs[0].source.owner_name = Some("changed-owner".to_owned());
    }
    assert!(
        crate::launch::classify_owned_terminal_available(
            scoped,
            SET_ID,
            &session.session_id,
            &changed_source,
        )
        .await
        .map_err(|error| error.to_string())?
        .is_none()
    );
    Ok(())
}

async fn assert_acquired_only_defers(
    link: &mut crate::listen::Link,
    session: &mut velnor_runner_github::QueueSession,
    scoped: &crate::journal::ConnectionJournal,
    poll_session: &PollSession,
    admission_calls: &std::cell::Cell<usize>,
    acknowledgement_calls: &std::cell::Cell<usize>,
    witness_request_id: i64,
    acquire_calls_before: usize,
) -> Result<(), String> {
    let acquired_only = poll_once_with_test_effects(
        link,
        session,
        scoped,
        poll_session,
        available_poll(131, witness_request_id)?,
        false,
        admission_calls,
        acknowledgement_calls,
    )
    .await
    .map_err(|error| error.to_string())?;
    assert_eq!(admission_calls.get(), 1);
    assert_eq!(acknowledgement_calls.get(), 1);
    assert!(matches!(
        acquired_only.admission,
        Some(crate::launch::PositiveAdmission::NoAcquire)
    ));
    assert_eq!(
        acquired_only.acknowledgement,
        crate::launch::Acknowledgement::Deferred
    );
    assert_eq!(acquired_only.settlement, crate::launch::Settlement::Settled);
    assert_eq!(poll_session.acquire_calls.get(), acquire_calls_before);
    Ok(())
}

async fn poll_once_with_test_effects(
    link: &mut crate::listen::Link,
    session: &mut velnor_runner_github::QueueSession,
    scoped: &crate::journal::ConnectionJournal,
    poll_session: &PollSession,
    polled: velnor_runner_github::Poll,
    expect_terminal: bool,
    admission_calls: &std::cell::Cell<usize>,
    acknowledgement_calls: &std::cell::Cell<usize>,
) -> Result<crate::launch::PositivePollOutcome, crate::scale_set::EnsureError> {
    crate::launch::test_positive_poll_once_with_response(
        link,
        SET_ID,
        session,
        scoped,
        poll_session.intent_id,
        &poll_session.allocation,
        0,
        polled,
        "https://queue.example.invalid/messages".to_owned(),
        Some("queue-token".to_owned()),
        async |_link, _session, path, queue, _polled| {
            assert_eq!(path, "https://queue.example.invalid/messages");
            assert_eq!(queue.as_deref(), Some("queue-token"));
            acknowledgement_calls.set(acknowledgement_calls.get() + 1);
            Ok(())
        },
        async |_link, _polled, _queue, _path, _queue_token, _admin_token| {
            admission_calls.set(admission_calls.get() + 1);
            if expect_terminal {
                Err(crate::scale_set::EnsureError::Uncertain)
            } else {
                Ok(crate::launch::PositiveAdmission::NoAcquire)
            }
        },
    )
    .await
}

fn available_poll(message_id: i64, request_id: i64) -> Result<velnor_runner_github::Poll, String> {
    let body = serde_json::json!([{
        "messageType": "JobAvailable",
        "runnerRequestId": request_id,
        "jobId": NATIVE_COMPOSITION_JOB_ID,
        "ownerName": "owner",
        "repositoryName": "repository",
        "jobWorkflowRef": "owner/repository/.github/workflows/build.yml@refs/heads/main",
        "workflowRunId": 900,
        "eventName": "push"
    }])
    .to_string();
    let envelope = serde_json::json!({
        "messageId": message_id,
        "messageType": "RunnerScaleSetJobMessages",
        "body": body
    })
    .to_string();
    velnor_runner_github::parse_poll(200, &envelope).map_err(|error| error.to_string())
}
