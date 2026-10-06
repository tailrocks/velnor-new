//! Runner lookup and removal against the pinned distributed-task API.

use velnor_runner_github::{
    Certainty, Method, RunnerReference, SessionError, TransportFail, WireError, get_runner_by_name,
    remove_runner,
};

mod common;

use common::{Script, header};

const ADMIN: &str = "runner-admin-canary";
const RUNNER: &str = r#"{"id":31,"name":"runner-31","runnerScaleSetId":7}"#;

fn failed<T>(result: &Result<T, SessionError>) -> Result<SessionError, &'static str> {
    match result {
        Ok(_) => Err("expected error"),
        Err(error) => Ok(*error),
    }
}

#[test]
fn get_runner_by_name_uses_agent_name_and_preserves_reference() -> Result<(), String> {
    let mut script = Script::once(200, &format!(r#"{{"count":1,"value":[{RUNNER}]}}"#));
    let runner = get_runner_by_name(&mut script, "runner a/+", ADMIN)
        .map_err(|error| format!("{error:?}"))?
        .ok_or("runner")?;
    assert_eq!(
        runner,
        RunnerReference {
            id: 31,
            name: "runner-31".to_owned(),
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
fn only_count_zero_is_absent_and_ambiguous_results_fail() -> Result<(), String> {
    let mut absent = Script::once(200, r#"{"count":0,"value":[]}"#);
    assert_eq!(
        get_runner_by_name(&mut absent, "runner-31", ADMIN).map_err(|err| format!("{err:?}"))?,
        None
    );

    let multiple = format!(r#"{{"count":2,"value":[{RUNNER},{RUNNER}]}}"#);
    let mut many = Script::once(200, &multiple);
    assert_eq!(
        failed(&get_runner_by_name(&mut many, "runner-31", ADMIN)),
        Ok(SessionError::Wire(WireError::MultipleResults))
    );

    for body in [
        r#"{"count":1,"value":[]}"#,
        r#"{"count":0,"value":[{"id":31,"name":"runner-31","runnerScaleSetId":7}]}"#,
        r#"{"count":-1,"value":[]}"#,
        r#"{"count":1,"value":[{"id":31,"name":"runner-31"}]}"#,
        "not json",
    ] {
        let mut malformed = Script::once(200, body);
        assert_eq!(
            failed(&get_runner_by_name(&mut malformed, "runner-31", ADMIN)),
            Ok(SessionError::Wire(WireError::Malformed))
        );
    }
    Ok(())
}

#[test]
fn get_runner_by_name_distinguishes_http_errors_from_transport_uncertainty() {
    for status in [404, 500] {
        let mut script = Script::once(status, "ignored-body");
        let error = failed(&get_runner_by_name(&mut script, "runner-31", ADMIN));
        assert_eq!(error, Ok(SessionError::Wire(WireError::UnexpectedStatus)));
        assert_eq!(
            error.ok().map(SessionError::certainty),
            Some(Certainty::Definite)
        );
    }
    let mut forbidden = Script::once(403, "");
    let error = failed(&get_runner_by_name(&mut forbidden, "runner-31", ADMIN));
    assert_eq!(error, Ok(SessionError::Wire(WireError::Forbidden)));
    assert_eq!(
        error.ok().map(SessionError::certainty),
        Some(Certainty::Definite)
    );

    for failure in [TransportFail::Timeout, TransportFail::Reset] {
        let mut script = if failure == TransportFail::Timeout {
            Script::fail(failure)
        } else {
            Script::replies(vec![Err(failure)])
        };
        let error = failed(&get_runner_by_name(&mut script, "runner-31", ADMIN));
        assert_eq!(error, Ok(SessionError::Uncertain));
        assert_eq!(
            error.ok().map(SessionError::certainty),
            Some(Certainty::Uncertain)
        );
    }
}

#[test]
fn remove_runner_requires_http_204() -> Result<(), String> {
    let mut removed = Script::once(204, "");
    remove_runner(&mut removed, 31, ADMIN).map_err(|error| format!("{error:?}"))?;
    let request = removed.seen.first().ok_or("request")?;
    assert_eq!(request.method, Method::Delete);
    assert_eq!(request.path, "_apis/distributedtask/pools/0/agents/31");
    assert_eq!(request.query.as_deref(), Some("api-version=6.0-preview"));
    assert_eq!(
        header(request, "Authorization"),
        Some("Bearer runner-admin-canary")
    );

    for status in [200, 404, 500] {
        let mut script = Script::once(status, "ignored-body");
        let error = failed(&remove_runner(&mut script, 31, ADMIN));
        assert_eq!(error, Ok(SessionError::Wire(WireError::UnexpectedStatus)));
        assert_eq!(
            error.ok().map(SessionError::certainty),
            Some(Certainty::Definite)
        );
    }
    let mut script = Script::once(403, "");
    assert_eq!(
        failed(&remove_runner(&mut script, 31, ADMIN)),
        Ok(SessionError::Wire(WireError::Forbidden))
    );
    let mut uncertain = Script::replies(vec![Err(TransportFail::Timeout)]);
    assert_eq!(
        failed(&remove_runner(&mut uncertain, 31, ADMIN)),
        Ok(SessionError::Uncertain)
    );
    Ok(())
}
