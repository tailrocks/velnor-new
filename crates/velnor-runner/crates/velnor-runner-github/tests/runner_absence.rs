//! Exact runner absence and removal protocol receipts.

use velnor_runner_github::{
    Method, RunnerReference, RunnerRemovalStatus, SessionError, TransportFail, WireError,
    ensure_runner_absent,
};

#[expect(
    dead_code,
    reason = "Shared fixture helpers serve other GitHub integration tests."
)]
mod common;

use common::Script;

const ADMIN: &str = "runner-admin-canary";

#[test]
fn absence_receipt_accepts_only_a_verified_empty_exact_lookup() -> Result<(), String> {
    let expected = runner(31, "runner-31", 7);
    let mut script = Script::once(200, r#"{"count":0,"value":[]}"#);
    let receipt = ensure_runner_absent(&mut script, &expected, ADMIN)
        .map_err(|failure| format!("{failure:?}"))?;

    assert_eq!(receipt.runner(), &expected);
    assert_eq!(receipt.removal_status(), RunnerRemovalStatus::NotNeeded);
    assert_eq!(receipt.responses().len(), 1);
    let response = &receipt.responses()[0];
    assert_eq!(response.status(), Some(200));
    assert!(response.status_verified());
    assert_eq!(response.count(), Some(0));
    assert_eq!(response.value_count(), Some(0));
    assert!(response.observed_at_unix_ms() > 0);
    assert_eq!(response.request_sha256().len(), 64);
    assert_eq!(response.response_sha256().map(str::len), Some(64));
    assert!(!format!("{receipt:?}").contains(ADMIN));
    assert_eq!(script.seen.len(), 1);
    Ok(())
}

#[test]
fn absence_receipt_removes_only_the_exact_runner_then_confirms_empty_lookup() -> Result<(), String>
{
    let expected = runner(31, "runner-31", 7);
    let mut script = Script::replies(vec![
        Ok(common::exchange(
            200,
            r#"{"count":1,"value":[{"id":31,"name":"runner-31","runnerScaleSetId":7}]}"#,
        )),
        Ok(common::exchange(204, "")),
        Ok(common::exchange(200, r#"{"count":0,"value":[]}"#)),
    ]);
    let receipt = ensure_runner_absent(&mut script, &expected, ADMIN)
        .map_err(|failure| format!("{failure:?}"))?;

    assert_eq!(receipt.removal_status(), RunnerRemovalStatus::Removed);
    assert_eq!(receipt.responses().len(), 3);
    assert_eq!(
        receipt.responses()[1].operation(),
        velnor_runner_github::RunnerObservationOperation::RemoveById
    );
    assert_eq!(receipt.responses()[1].status(), Some(204));
    assert!(receipt.responses()[1].status_verified());
    assert_eq!(script.seen[0].method, Method::Get);
    assert_eq!(script.seen[1].method, Method::Delete);
    assert_eq!(
        script.seen[1].path,
        "_apis/distributedtask/pools/0/agents/31"
    );
    assert_eq!(script.seen[2].method, Method::Get);
    assert_eq!(
        script.seen[0].query.as_deref(),
        Some("agentName=runner-31&api-version=6.0-preview")
    );
    assert_eq!(
        script.seen[2].query.as_deref(),
        Some("agentName=runner-31&api-version=6.0-preview")
    );
    Ok(())
}

#[test]
fn absence_receipt_rejects_identity_mismatch_and_malformed_counts_before_delete() {
    let expected = runner(31, "runner-31", 7);
    let mut mismatch = Script::once(
        200,
        r#"{"count":1,"value":[{"id":32,"name":"runner-31","runnerScaleSetId":7}]}"#,
    );
    assert_eq!(
        ensure_runner_absent(&mut mismatch, &expected, ADMIN),
        Err(SessionError::Wire(WireError::RegistrationRejected))
    );
    assert_eq!(mismatch.seen.len(), 1);

    let mut wrong_scale_set = Script::once(
        200,
        r#"{"count":1,"value":[{"id":31,"name":"runner-31","runnerScaleSetId":8}]}"#,
    );
    assert_eq!(
        ensure_runner_absent(&mut wrong_scale_set, &expected, ADMIN),
        Err(SessionError::Wire(WireError::RegistrationRejected))
    );
    assert_eq!(wrong_scale_set.seen.len(), 1);

    let mut malformed = Script::once(200, r#"{"count":1,"value":[]}"#);
    assert_eq!(
        ensure_runner_absent(&mut malformed, &expected, ADMIN),
        Err(SessionError::Wire(WireError::Malformed))
    );
    assert_eq!(malformed.seen.len(), 1);

    for (status, expected_error) in [
        (403, SessionError::Wire(WireError::Forbidden)),
        (404, SessionError::Wire(WireError::UnexpectedStatus)),
    ] {
        let mut rejected_lookup = Script::once(status, "ignored");
        assert_eq!(
            ensure_runner_absent(&mut rejected_lookup, &expected, ADMIN),
            Err(expected_error)
        );
        assert_eq!(rejected_lookup.seen.len(), 1);
    }
}

#[test]
fn absence_receipt_handles_forbidden_not_found_and_uncertain_removal() -> Result<(), String> {
    let expected = runner(31, "runner-31", 7);
    let lookup = || {
        Ok(common::exchange(
            200,
            r#"{"count":1,"value":[{"id":31,"name":"runner-31","runnerScaleSetId":7}]}"#,
        ))
    };

    let mut forbidden = Script::replies(vec![lookup(), Ok(common::exchange(403, ""))]);
    assert_eq!(
        ensure_runner_absent(&mut forbidden, &expected, ADMIN),
        Err(SessionError::Wire(WireError::Forbidden))
    );
    assert_eq!(forbidden.seen.len(), 2);

    let mut not_found = Script::replies(vec![
        lookup(),
        Err(TransportFail::Http(404)),
        Ok(common::exchange(200, r#"{"count":0,"value":[]}"#)),
    ]);
    let absent = ensure_runner_absent(&mut not_found, &expected, ADMIN)
        .map_err(|failure| format!("{failure:?}"))?;
    assert_eq!(
        absent.removal_status(),
        RunnerRemovalStatus::AbsentAfterRejectedRemoval
    );
    assert_eq!(absent.responses()[1].status(), Some(404));
    assert!(!absent.responses()[1].status_verified());
    assert!(absent.responses()[1].response_sha256().is_some());

    let mut uncertain = Script::replies(vec![
        lookup(),
        Err(TransportFail::Timeout),
        Ok(common::exchange(200, r#"{"count":0,"value":[]}"#)),
    ]);
    let absent = ensure_runner_absent(&mut uncertain, &expected, ADMIN)
        .map_err(|failure| format!("{failure:?}"))?;
    assert_eq!(
        absent.removal_status(),
        RunnerRemovalStatus::AbsentAfterUncertainRemoval
    );
    assert_eq!(absent.responses()[1].status(), None);
    assert_eq!(absent.responses()[1].response_sha256(), None);
    assert_eq!(uncertain.seen.len(), 3);
    Ok(())
}

#[test]
fn absence_receipt_rejects_a_runner_that_remains_after_successful_delete() {
    let expected = runner(31, "runner-31", 7);
    let mut still_present = Script::replies(vec![
        Ok(runner_lookup(31, 7)),
        Ok(common::exchange(204, "")),
        Ok(runner_lookup(31, 7)),
    ]);
    assert_eq!(
        ensure_runner_absent(&mut still_present, &expected, ADMIN),
        Err(SessionError::Uncertain)
    );
    assert_eq!(still_present.seen.len(), 3);
    assert_eq!(still_present.seen[1].method, Method::Delete);
}

#[test]
fn absence_receipt_rejects_every_unverified_final_lookup() {
    let expected = runner(31, "runner-31", 7);
    let confirmations = [
        (Err(TransportFail::Timeout), SessionError::Uncertain),
        (Err(TransportFail::Reset), SessionError::Uncertain),
        (
            Err(TransportFail::Http(403)),
            SessionError::Wire(WireError::Forbidden),
        ),
        (
            Err(TransportFail::Http(404)),
            SessionError::Wire(WireError::UnexpectedStatus),
        ),
        (
            Ok(common::exchange(200, "malformed")),
            SessionError::Wire(WireError::Malformed),
        ),
    ];
    for (confirmation, expected_error) in confirmations {
        let mut script = Script::replies(vec![
            Ok(runner_lookup(31, 7)),
            Ok(common::exchange(204, "")),
            confirmation,
        ]);
        assert_eq!(
            ensure_runner_absent(&mut script, &expected, ADMIN),
            Err(expected_error)
        );
        assert_eq!(script.seen.len(), 3);
    }
}

#[test]
fn absence_receipt_rejects_replacement_identity_after_delete() {
    let expected = runner(31, "runner-31", 7);
    for (replacement_id, replacement_scale_set) in [(32, 7), (31, 8)] {
        let mut replacement = Script::replies(vec![
            Ok(runner_lookup(31, 7)),
            Ok(common::exchange(204, "")),
            Ok(runner_lookup(replacement_id, replacement_scale_set)),
        ]);
        assert_eq!(
            ensure_runner_absent(&mut replacement, &expected, ADMIN),
            Err(SessionError::Wire(WireError::RegistrationRejected))
        );
        assert_eq!(replacement.seen.len(), 3);
    }
}

#[test]
fn canonical_request_digest_excludes_authorization_token() -> Result<(), String> {
    let expected = runner(31, "runner-31", 7);
    let mut first = Script::once(200, r#"{"count":0,"value":[]}"#);
    let first_receipt = ensure_runner_absent(&mut first, &expected, "token-one")
        .map_err(|failure| format!("{failure:?}"))?;
    let mut second = Script::once(200, r#"{"count":0,"value":[]}"#);
    let second_receipt = ensure_runner_absent(&mut second, &expected, "token-two")
        .map_err(|failure| format!("{failure:?}"))?;
    assert_eq!(
        first_receipt.responses()[0].request_sha256(),
        second_receipt.responses()[0].request_sha256()
    );
    assert!(!format!("{first_receipt:?}").contains("token-one"));
    assert!(!format!("{second_receipt:?}").contains("token-two"));

    let mut response_with_secret = Script::once(
        200,
        r#"{"count":0,"value":[],"unexpectedCredential":"response-secret"}"#,
    );
    let secret_receipt = ensure_runner_absent(&mut response_with_secret, &expected, ADMIN)
        .map_err(|failure| format!("{failure:?}"))?;
    assert_eq!(
        first_receipt.responses()[0].response_sha256(),
        secret_receipt.responses()[0].response_sha256()
    );
    assert!(!format!("{secret_receipt:?}").contains("response-secret"));
    Ok(())
}

fn runner(id: i64, name: &str, scale_set_id: i64) -> RunnerReference {
    RunnerReference {
        id,
        name: name.to_owned(),
        runner_scale_set_id: scale_set_id,
    }
}

fn runner_lookup(id: i64, scale_set_id: i64) -> velnor_runner_github::Exchange {
    let body = format!(
        r#"{{"count":1,"value":[{{"id":{id},"name":"runner-31","runnerScaleSetId":{scale_set_id}}}]}}"#
    );
    common::exchange(200, &body)
}
