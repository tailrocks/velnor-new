//! Runner lookup and removal follow the pinned distributed-task API.

use velnor_runner_github::{
    Certainty, Method, RunnerReference, SessionError, TransportFail, WireError, get_runner_by_name,
    remove_runner,
};

mod common;

use common::{Script, header};

const ADMIN: &str = "runner-admin-canary";

fn error<T>(result: &Result<T, SessionError>) -> Result<SessionError, &'static str> {
    match result {
        Ok(_) => Err("expected error"),
        Err(found) => Ok(*found),
    }
}

#[test]
fn get_runner_by_name_uses_exact_query_and_decodes_reference() -> Result<(), String> {
    let body = r#"{"count":1,"value":[{"id":31,"name":"runner a/+","runnerScaleSetId":7}]}"#;
    let mut script = Script::once(200, body);
    let runner = get_runner_by_name(&mut script, "runner a/+", ADMIN)
        .map_err(|failure| format!("{failure:?}"))?
        .ok_or("runner")?;
    assert_eq!(
        runner,
        RunnerReference {
            id: 31,
            name: "runner a/+".to_owned(),
            runner_scale_set_id: 7,
        }
    );
    let request = script.seen.first().ok_or("request")?;
    assert_eq!(request.method, Method::Get);
    assert_eq!(request.path, "_apis/distributedtask/pools/0/agents");
    assert_ne!(request.path, common::QUEUE);
    assert_eq!(
        request.query.as_deref(),
        Some("agentName=runner+a%2F%2B&api-version=6.0-preview")
    );
    assert_eq!(header(request, "User-Agent"), Some("velnor-host"));
    assert_eq!(
        header(request, "Authorization"),
        Some("Bearer runner-admin-canary")
    );
    assert!(!format!("{request:?}").contains(ADMIN));
    Ok(())
}

#[test]
fn only_an_exact_single_result_is_returned() {
    let mut absent = Script::once(200, r#"{"count":0,"value":[]}"#);
    assert_eq!(
        get_runner_by_name(&mut absent, "runner-31", ADMIN),
        Ok(None)
    );

    let many = r#"{"count":2,"value":[{"id":31,"name":"runner-31","runnerScaleSetId":7},{"id":32,"name":"runner-31","runnerScaleSetId":7}]}"#;
    let mut duplicate = Script::once(200, many);
    assert_eq!(
        error(&get_runner_by_name(&mut duplicate, "runner-31", ADMIN)),
        Ok(SessionError::Wire(WireError::MultipleResults))
    );

    for body in [
        r#"{"count":0}"#,
        r#"{"count":1,"value":[]}"#,
        r#"{"count":0,"value":[{"id":31,"name":"runner-31","runnerScaleSetId":7}]}"#,
        r#"{"count":1,"value":[{"id":31,"name":"other","runnerScaleSetId":7}]}"#,
        r#"{"count":1,"value":[{"id":0,"name":"runner-31","runnerScaleSetId":7}]}"#,
        r#"{"count":1,"value":[{"id":31,"name":"runner-31","runnerScaleSetId":0}]}"#,
        "not json",
    ] {
        let mut malformed = Script::once(200, body);
        assert_eq!(
            error(&get_runner_by_name(&mut malformed, "runner-31", ADMIN)),
            Ok(SessionError::Wire(WireError::Malformed))
        );
    }
}

#[test]
fn lookup_errors_keep_http_and_transport_certainty() {
    for (status, expected, certainty) in [
        (
            403,
            SessionError::Wire(WireError::Forbidden),
            Certainty::Definite,
        ),
        (
            404,
            SessionError::Wire(WireError::UnexpectedStatus),
            Certainty::Uncertain,
        ),
        (
            500,
            SessionError::Wire(WireError::UnexpectedStatus),
            Certainty::Uncertain,
        ),
    ] {
        let mut script = Script::once(status, "ignored");
        let found = error(&get_runner_by_name(&mut script, "runner-31", ADMIN));
        assert_eq!(found, Ok(expected));
        assert_eq!(found.ok().map(SessionError::certainty), Some(certainty));
    }
    for failure in [TransportFail::Timeout, TransportFail::Reset] {
        let mut script = Script::fail(failure);
        let found = error(&get_runner_by_name(&mut script, "runner-31", ADMIN));
        assert_eq!(found, Ok(SessionError::Uncertain));
        assert_eq!(
            found.ok().map(SessionError::certainty),
            Some(Certainty::Uncertain)
        );
    }
}

#[test]
fn invalid_lookup_and_delete_inputs_do_not_reach_transport() {
    let mut lookup = Script::once(200, r#"{"count":0,"value":[]}"#);
    assert_eq!(
        error(&get_runner_by_name(&mut lookup, "", ADMIN)),
        Ok(SessionError::Wire(WireError::RegistrationRejected))
    );
    assert_eq!(
        lookup.seen,
        [] as [velnor_runner_github::SessionRequest; 0]
    );

    for runner_id in [0, -1] {
        let mut remove = Script::once(204, "");
        assert_eq!(
            error(&remove_runner(&mut remove, runner_id, ADMIN)),
            Ok(SessionError::Wire(WireError::RegistrationRejected))
        );
        assert_eq!(
            remove.seen,
            [] as [velnor_runner_github::SessionRequest; 0]
        );
    }
}

#[test]
fn remove_runner_uses_id_and_requires_http_204() -> Result<(), String> {
    let mut removed = Script::once(204, "");
    remove_runner(&mut removed, 31, ADMIN).map_err(|failure| format!("{failure:?}"))?;
    let request = removed.seen.first().ok_or("request")?;
    assert_eq!(request.method, Method::Delete);
    assert_eq!(request.path, "_apis/distributedtask/pools/0/agents/31");
    assert_eq!(request.query.as_deref(), Some("api-version=6.0-preview"));
    assert_eq!(
        header(request, "Authorization"),
        Some("Bearer runner-admin-canary")
    );

    for status in [200, 404, 500] {
        let mut script = Script::once(status, "ignored");
        assert_eq!(
            error(&remove_runner(&mut script, 31, ADMIN)),
            Ok(SessionError::Wire(WireError::UnexpectedStatus))
        );
    }
    let mut forbidden = Script::once(403, "");
    assert_eq!(
        error(&remove_runner(&mut forbidden, 31, ADMIN)),
        Ok(SessionError::Wire(WireError::Forbidden))
    );
    let mut uncertain = Script::fail(TransportFail::Timeout);
    assert_eq!(
        error(&remove_runner(&mut uncertain, 31, ADMIN)),
        Ok(SessionError::Uncertain)
    );
    Ok(())
}
