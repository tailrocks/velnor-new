use std::path::Path;

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

    fn read_staged_reports(
        &self,
        run_key: &str,
        plan: &serde_json::Value,
        dir: &Path,
        errors: &mut Vec<String>,
    ) -> (Vec<serde_json::Value>, Vec<serde_json::Value>) {
        assert_eq!(run_key, "stub-run");
        assert!(plan.is_object());
        assert!(dir.ends_with("reports"));
        errors.push("stub_gap".to_owned());
        (vec![serde_json::json!({"id": "r1"})], Vec::new())
    }
}

#[test]
fn port_explicit_run_key_wins_over_stub_default() {
    let port = StubRequest {
        run_key: Ok("stub-default".to_owned()),
    };
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
fn port_dispatches_both_methods_through_one_dyn_reference() {
    let port = StubRequest {
        run_key: Ok("stub-run".to_owned()),
    };
    let port: &dyn RequestPort = &port;
    let key = port.resolve_run_key(None).expect("key");
    let mut errors = Vec::new();
    let (reports, _) = port.read_staged_reports(
        &key,
        &serde_json::json!({"matrix": {"include": []}}),
        Path::new("/tmp/stub/reports"),
        &mut errors,
    );
    assert_eq!(reports.len(), 1);
    assert_eq!(errors.len(), 1);
}

#[test]
fn port_dispatches_through_boxed_trait_object() {
    let port: Box<dyn RequestPort> = Box::new(StubRequest {
        run_key: Ok("boxed".to_owned()),
    });
    assert_eq!(port.resolve_run_key(None).expect("key"), "boxed");
}

#[test]
fn port_allows_distinct_implementations_side_by_side() {
    let first = StubRequest {
        run_key: Ok("first".to_owned()),
    };
    let second = StubRequest {
        run_key: Ok("second".to_owned()),
    };
    let ports: [&dyn RequestPort; 2] = [&first, &second];
    let keys: Vec<String> = ports
        .iter()
        .map(|port| port.resolve_run_key(None).expect("key"))
        .collect();
    assert_eq!(keys, vec!["first".to_owned(), "second".to_owned()]);
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
    let port = StubRequest {
        run_key: Ok("stub-default".to_owned()),
    };
    let port: &dyn RequestPort = &port;
    assert_eq!(port.resolve_run_key(Some("")).expect("key"), "");
}

#[test]
fn port_reports_and_tasks_are_independent_lists() {
    struct Split;
    impl RequestPort for Split {
        fn resolve_run_key(&self, _explicit: Option<&str>) -> Result<String, OrchestratorError> {
            Ok("split".to_owned())
        }

        fn read_staged_reports(
            &self,
            _run_key: &str,
            _plan: &serde_json::Value,
            _dir: &Path,
            _errors: &mut Vec<String>,
        ) -> (Vec<serde_json::Value>, Vec<serde_json::Value>) {
            (
                vec![
                    serde_json::json!({"id": "r1"}),
                    serde_json::json!({"id": "r2"}),
                ],
                vec![serde_json::json!({"id": "t1"})],
            )
        }
    }

    let port = Split;
    let port: &dyn RequestPort = &port;
    let mut errors = Vec::new();
    let (reports, tasks) = port.read_staged_reports(
        "split",
        &serde_json::json!({}),
        Path::new("/tmp/split/reports"),
        &mut errors,
    );
    assert_eq!(reports.len(), 2);
    assert_eq!(tasks.len(), 1);
    assert!(errors.is_empty());
}

#[test]
fn port_caller_owns_errors_across_calls() {
    let port = StubRequest {
        run_key: Ok("stub-run".to_owned()),
    };
    let port: &dyn RequestPort = &port;
    let mut errors = vec!["prior".to_owned()];
    let _ = port.read_staged_reports(
        "stub-run",
        &serde_json::json!({"matrix": {"include": []}}),
        Path::new("/tmp/stub/reports"),
        &mut errors,
    );
    assert_eq!(errors, vec!["prior".to_owned(), "stub_gap".to_owned()]);
}

#[test]
fn port_staged_reports_record_gaps_explicitly() {
    let port = StubRequest {
        run_key: Ok("stub-run".to_owned()),
    };
    let port: &dyn RequestPort = &port;
    let mut errors = Vec::new();
    let (reports, tasks) = port.read_staged_reports(
        "stub-run",
        &serde_json::json!({"matrix": {"include": []}}),
        Path::new("/tmp/stub/reports"),
        &mut errors,
    );
    assert_eq!(reports.len(), 1);
    assert!(tasks.is_empty());
    assert_eq!(errors, vec!["stub_gap".to_owned()]);
}
