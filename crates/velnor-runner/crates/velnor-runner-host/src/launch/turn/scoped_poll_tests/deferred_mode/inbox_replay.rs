use std::collections::BTreeMap;

use velnor_runner_github::{ParsedBatch, Poll, parse_inner_messages};

use super::*;

const PAGE_NOW: i64 = 4_102_444_800;
const AVAILABLE_REQUEST: i64 = 9001;
const MIXED_MESSAGE: i64 = 1_000;
const SCALE_SET: i64 = 10_101;

#[tokio::test]
async fn deferred_mixed_inbox_survives_reopen_and_replays_exact_offer() -> Result<(), String> {
    let scratch = Scratch::new()?;
    let (scope, _, _) = session_scope(&scratch).await?;
    let binding = repository_connection()?;
    let backlog = insert_backlog(&scope).await?;
    assert!(backlog.len() > crate::journal::MAX_COMPLETION_INBOX_SCAN as usize);
    let raw_body = mixed_body();
    let polled = mixed_poll(&raw_body)?;
    let receipts = durable_offer_set(&polled, &scope, SCALE_SET, AVAILABLE_REQUEST).await?;
    let (first_completion_only, first_acknowledged) =
        process_and_check_ack(&scope, &polled, receipts, &raw_body, PAGE_NOW).await?;
    assert!(first_acknowledged);
    assert!(!first_completion_only);

    drop(scope);
    let reopened = crate::journal::Journal::open(&scratch.file())
        .await
        .map_err(|error| error.to_string())?;
    let scope = reopened
        .connection_scope(&binding)
        .await
        .map_err(|error| error.to_string())?;
    assert!(
        scope
            .store_completion_inbox(MIXED_MESSAGE, "changed after reopen")
            .await
            .is_err()
    );
    assert_eq!(
        snapshot_all_bodies(&scope, PAGE_NOW + 1_000).await?,
        expected_bodies(&backlog, &raw_body)
    );

    let replay_receipts = durable_offer_set(&polled, &scope, SCALE_SET, AVAILABLE_REQUEST).await?;
    let (replay_completion_only, replay_acknowledged) = process_and_check_ack(
        &scope,
        &polled,
        replay_receipts,
        &raw_body,
        PAGE_NOW + 2_000,
    )
    .await?;

    assert!(replay_acknowledged);
    assert!(!replay_completion_only);
    assert_single_unacquired_offer(&scope).await?;
    drop(scope);
    drop(reopened);
    Ok(())
}

async fn insert_backlog(
    scope: &crate::journal::ConnectionJournal,
) -> Result<Vec<(i64, String)>, String> {
    let mut bodies = Vec::new();
    for index in 0..=128 {
        let message_id = 10 + i64::from(index);
        let body = format!("unparsed backlog body {message_id}");
        scope
            .store_completion_inbox(message_id, &body)
            .await
            .map_err(|error| error.to_string())?;
        bodies.push((message_id, body));
    }
    Ok(bodies)
}

async fn process_and_check_ack(
    scope: &crate::journal::ConnectionJournal,
    polled: &Poll,
    receipts: crate::launch::DeferredAuthorizedOffers,
    raw_body: &str,
    scan_now: i64,
) -> Result<(bool, bool), String> {
    let receipts = std::cell::RefCell::new(Some(receipts));
    let ack_saw_durable_state = std::cell::Cell::new(false);
    let (decision, acknowledged) = process_deferred_scoped_response(
        scope,
        SCALE_SET,
        polled,
        async |_| {
            receipts
                .borrow_mut()
                .take()
                .ok_or(crate::scale_set::EnsureError::AdmissionClosed)
        },
        async || {
            ack_saw_durable_state.set(ack_observes_body_and_offer(scope, raw_body, scan_now).await);
            if ack_saw_durable_state.get() {
                Ok(())
            } else {
                Err(crate::scale_set::EnsureError::AdmissionClosed)
            }
        },
    )
    .await
    .map_err(|error| format!("deferred intake failed: {error:?}"))?;
    if !ack_saw_durable_state.get() {
        return Err("ACK ran before exact inbox body and durable offer were visible".to_owned());
    }
    Ok((decision.completion_only, acknowledged))
}

async fn ack_observes_body_and_offer(
    scope: &crate::journal::ConnectionJournal,
    raw_body: &str,
    scan_now: i64,
) -> bool {
    let exact_body = find_persisted_body_by_bounded_pages(scope, scan_now, raw_body).await;
    let exact_offer = scope
        .authorized_offer(AVAILABLE_REQUEST, MIXED_MESSAGE)
        .await
        .is_ok_and(|offer| offer.is_some_and(|offer| offer.request_id() == AVAILABLE_REQUEST));
    exact_body && exact_offer
}

async fn find_persisted_body_by_bounded_pages(
    scope: &crate::journal::ConnectionJournal,
    now: i64,
    expected_body: &str,
) -> bool {
    loop {
        let Ok(entries) = scope
            .pending_completion_inbox(now, crate::journal::MAX_COMPLETION_INBOX_SCAN)
            .await
        else {
            return false;
        };
        if entries.is_empty() {
            return false;
        }
        for entry in entries {
            if entry.message_id == MIXED_MESSAGE {
                return entry.raw_body == expected_body;
            }
            if scope.defer_completion_inbox(&entry, now).await.is_err() {
                return false;
            }
        }
    }
}

async fn snapshot_all_bodies(
    scope: &crate::journal::ConnectionJournal,
    now: i64,
) -> Result<BTreeMap<i64, String>, String> {
    let mut bodies = BTreeMap::new();
    loop {
        let entries = scope
            .pending_completion_inbox(now, crate::journal::MAX_COMPLETION_INBOX_SCAN)
            .await
            .map_err(|error| error.to_string())?;
        if entries.is_empty() {
            return Ok(bodies);
        }
        for entry in entries {
            bodies.insert(entry.message_id, entry.raw_body.clone());
            scope
                .defer_completion_inbox(&entry, now)
                .await
                .map_err(|error| error.to_string())?;
        }
    }
}

fn expected_bodies(backlog: &[(i64, String)], latest: &str) -> BTreeMap<i64, String> {
    let mut expected = backlog.iter().cloned().collect::<BTreeMap<_, _>>();
    expected.insert(MIXED_MESSAGE, latest.to_owned());
    expected
}

fn mixed_body() -> String {
    format!(
        r#"[{{"messageType":"JobAvailable","runnerRequestId":{AVAILABLE_REQUEST},"jobId":"opaque-available"}},{{"messageType":"JobCompleted","runnerRequestId":99001,"runnerId":88,"runnerName":"runner-88","result":"Succeeded","jobId":"unmatched-completion"}}]"#
    )
}

fn mixed_poll(raw_body: &str) -> Result<Poll, String> {
    let jobs =
        parse_inner_messages(raw_body).map_err(|error| format!("bad fixture body: {error}"))?;
    Ok(Poll::Batch(ParsedBatch {
        message_id: MIXED_MESSAGE,
        raw_body: raw_body.to_owned(),
        statistics: None,
        jobs,
    }))
}

async fn assert_single_unacquired_offer(
    scope: &crate::journal::ConnectionJournal,
) -> Result<(), String> {
    use crate::journal::{AssignmentKey, RequestAcquireState};

    let offers = scope
        .authorized_offers(8)
        .await
        .map_err(|error| error.to_string())?;
    assert_eq!(offers.len(), 1);
    assert_eq!(offers[0].request_id(), AVAILABLE_REQUEST);
    assert_eq!(offers[0].message_id(), MIXED_MESSAGE);
    let identity = scope
        .assignment_identity(AssignmentKey::AssignedRequest {
            request_id: AVAILABLE_REQUEST,
        })
        .await
        .map_err(|error| error.to_string())?;
    assert_eq!(
        scope
            .request_acquire_state(&identity)
            .await
            .map_err(|error| error.to_string())?,
        Some(RequestAcquireState::Authorized)
    );
    assert!(
        scope
            .reservation(&identity)
            .await
            .map_err(|error| error.to_string())?
            .is_none()
    );
    Ok(())
}
