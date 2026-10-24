use super::*;

struct NativeReplayContext<'a> {
    link: &'a mut crate::listen::Link,
    session: &'a mut velnor_runner_github::QueueSession,
    scoped: &'a crate::journal::ConnectionJournal,
    poll_session: &'a PollSession,
    admission_calls: std::cell::Cell<usize>,
    acknowledgement_calls: std::cell::Cell<usize>,
    acquire_calls_before: usize,
}

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
    let mut context = NativeReplayContext {
        link: &mut link,
        session: &mut session,
        scoped: &fixture.scope,
        poll_session: &poll_session,
        admission_calls: std::cell::Cell::new(0),
        acknowledgement_calls: std::cell::Cell::new(0),
        acquire_calls_before: poll_session.acquire_calls.get(),
    };
    assert_terminal_replay(&mut context, actual_request_id).await?;
    assert_acquired_only_defers(&mut context, witness_request_id).await?;
    Ok(())
}

async fn assert_terminal_replay(
    context: &mut NativeReplayContext<'_>,
    actual_request_id: i64,
) -> Result<(), String> {
    let terminal_poll = available_poll(130, actual_request_id)?;
    let outcome = poll_once_with_test_effects(context, terminal_poll.clone(), true)
        .await
        .map_err(|error| error.to_string())?;
    assert_eq!(context.admission_calls.get(), 0);
    assert_eq!(context.acknowledgement_calls.get(), 1);
    let Some(crate::launch::PositiveAdmission::OwnedTerminal(receipt)) = outcome.admission.as_ref()
    else {
        return Err("The terminal replay receipt is absent.".to_owned());
    };
    assert!(receipt.matches_poll(
        &terminal_poll,
        context.scoped,
        SET_ID,
        &context.session.session_id
    ));
    assert!(!receipt.matches_poll(&terminal_poll, context.scoped, SET_ID, "changed-session"));
    assert_job_id_change_is_rejected(&terminal_poll, context.scoped, context.session).await?;
    assert_source_change_is_rejected(&terminal_poll, context.scoped, context.session).await?;
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
    assert_eq!(
        context.poll_session.acquire_calls.get(),
        context.acquire_calls_before
    );
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
    context: &mut NativeReplayContext<'_>,
    witness_request_id: i64,
) -> Result<(), String> {
    let acquired_only =
        poll_once_with_test_effects(context, available_poll(131, witness_request_id)?, false)
            .await
            .map_err(|error| error.to_string())?;
    assert_eq!(context.admission_calls.get(), 1);
    assert_eq!(context.acknowledgement_calls.get(), 1);
    assert!(matches!(
        acquired_only.admission,
        Some(crate::launch::PositiveAdmission::NoAcquire)
    ));
    assert_eq!(
        acquired_only.acknowledgement,
        crate::launch::Acknowledgement::Deferred
    );
    assert_eq!(acquired_only.settlement, crate::launch::Settlement::Settled);
    assert_eq!(
        context.poll_session.acquire_calls.get(),
        context.acquire_calls_before
    );
    Ok(())
}

async fn poll_once_with_test_effects(
    context: &mut NativeReplayContext<'_>,
    polled: velnor_runner_github::Poll,
    expect_terminal: bool,
) -> Result<crate::launch::PositivePollOutcome, crate::scale_set::EnsureError> {
    let link = &mut *context.link;
    let session = &mut *context.session;
    let scoped = context.scoped;
    let poll_session = context.poll_session;
    let admission_calls = &context.admission_calls;
    let acknowledgement_calls = &context.acknowledgement_calls;
    crate::launch::test_positive_poll_once_with_response(
        crate::launch::PositivePollContext {
            link,
            set_id: SET_ID,
            session,
            scoped,
            session_intent_id: poll_session.intent_id,
            allocation: &poll_session.allocation,
            cursor: 0,
        },
        crate::launch::TestPollResponse {
            polled,
            path: "https://queue.example.invalid/messages".to_owned(),
            queue: Some("queue-token".to_owned()),
        },
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
