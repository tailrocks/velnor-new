use velnor_actions_orchestrator_core::{OrchestratorError, internal};
use velnor_actions_orchestrator_merge_request_ports::RequestPort;

struct StubRequest {
    run_key: Result<String, String>,
}

impl RequestPort for StubRequest {
    fn resolve_run_key(&self, explicit: Option<&str>) -> Result<String, OrchestratorError> {
        if let Some(key) = explicit {
            return Ok(key.to_owned());
        }
        self.run_key.clone().map_err(|problem| internal(&problem))
    }
}

fn stub(key: &str) -> StubRequest {
    StubRequest {
        run_key: Ok(key.to_owned()),
    }
}

#[test]
fn port_explicit_run_key_wins_over_stub_default() {
    let port = stub("stub-default");
    let port: &dyn RequestPort = &port;
    assert_eq!(
        port.resolve_run_key(Some("explicit")).expect("key"),
        "explicit"
    );
    assert_eq!(port.resolve_run_key(None).expect("key"), "stub-default");
}

#[test]
fn port_run_key_failure_is_internal() {
    let port = StubRequest {
        run_key: Err("missing_run_key".to_owned()),
    };
    let err = port.resolve_run_key(None).expect_err("must fail");
    assert!(matches!(err, OrchestratorError::Internal { .. }));
}

#[test]
fn port_run_key_error_carries_problem_text() {
    let port = StubRequest {
        run_key: Err("missing_run_key".to_owned()),
    };
    let err = port.resolve_run_key(None).expect_err("must fail");
    assert!(
        err.to_string().contains("missing_run_key"),
        "unexpected: {err}"
    );
}

#[test]
fn port_empty_explicit_key_passes_through_verbatim() {
    let port: &dyn RequestPort = &stub("stub-default");
    assert_eq!(port.resolve_run_key(Some("")).expect("key"), "");
}

#[test]
fn port_whitespace_explicit_key_passes_through_verbatim() {
    let port: &dyn RequestPort = &stub("stub-default");
    assert_eq!(port.resolve_run_key(Some("  ")).expect("key"), "  ");
}

#[test]
fn port_long_explicit_key_passes_through_verbatim() {
    let port: &dyn RequestPort = &stub("stub-default");
    let long = "k".repeat(1024);
    assert_eq!(port.resolve_run_key(Some(&long)).expect("key"), long);
}

#[test]
fn port_dispatches_through_boxed_trait_object() {
    let port: Box<dyn RequestPort> = Box::new(stub("boxed"));
    assert_eq!(port.resolve_run_key(None).expect("key"), "boxed");
}

#[test]
fn port_allows_distinct_implementations_side_by_side() {
    let first = stub("first");
    let second = stub("second");
    let ports: [&dyn RequestPort; 2] = [&first, &second];
    let keys: Vec<String> = ports
        .iter()
        .map(|port| port.resolve_run_key(None).expect("key"))
        .collect();
    assert_eq!(keys, vec!["first".to_owned(), "second".to_owned()]);
}

#[test]
fn port_none_resolves_stub_default_repeatedly() {
    let port: &dyn RequestPort = &stub("steady");
    for _ in 0..3 {
        assert_eq!(port.resolve_run_key(None).expect("key"), "steady");
    }
}

#[test]
fn port_explicit_key_ignores_failing_default() {
    let port = StubRequest {
        run_key: Err("missing_run_key".to_owned()),
    };
    let port: &dyn RequestPort = &port;
    assert_eq!(
        port.resolve_run_key(Some("explicit")).expect("key"),
        "explicit"
    );
}
