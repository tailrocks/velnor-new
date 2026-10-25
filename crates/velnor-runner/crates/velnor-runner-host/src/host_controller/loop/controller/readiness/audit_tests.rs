use std::collections::VecDeque;

use velnor_runner_github::{Exchange, RunnerReference, SessionRequest, Transport, TransportFail};

use crate::{
    config::{HostConfig, RepositoryConnection},
    config_test_support::{basic_connection, host_config},
    error::HostError,
};

use super::{ReadinessTransport, audit_remote_runner_with_transport};

struct ScriptedTransport {
    responses: VecDeque<Exchange>,
    paths: Vec<String>,
    origin: String,
}

impl Transport for ScriptedTransport {
    fn exchange(&mut self, request: &SessionRequest) -> Result<Exchange, TransportFail> {
        self.paths.push(request.path.clone());
        self.responses.pop_front().ok_or(TransportFail::Http(500))
    }
}

impl ReadinessTransport for ScriptedTransport {
    fn set_origin(&mut self, base: &str) -> Result<(), HostError> {
        self.origin = base.to_owned();
        Ok(())
    }
}

fn binding() -> RepositoryConnection {
    let config = host_config(
        &[basic_connection(
            "essential",
            101,
            "donbeave/essential-mac",
            "essential",
            "essential-scale-set",
        )],
        1,
    );
    HostConfig::parse(&config)
        .expect("The exact test fixture connection must parse.")
        .connections()[0]
        .clone()
}

fn transport(runner_id: i64, runner_name: &str, runner_set_id: i64) -> ScriptedTransport {
    ScriptedTransport {
        responses: VecDeque::from([
            Exchange {
                status: 200,
                body: br#"{"id":101,"name":"essential-mac","full_name":"donbeave/essential-mac","owner":{"login":"donbeave"},"private":true}"#.to_vec(),
            },
            Exchange {
                status: 201,
                body: br#"{"token":"registration-token"}"#.to_vec(),
            },
            Exchange {
                status: 200,
                body: br#"{"url":"https://actions.example.test","token":"admin-token"}"#.to_vec(),
            },
            Exchange {
                status: 200,
                body: format!(
                    "{{\"count\":1,\"value\":[{{\"id\":{runner_id},\"name\":\"{runner_name}\",\"runnerScaleSetId\":{runner_set_id}}}]}}"
                )
                .into_bytes(),
            },
        ]),
        paths: Vec::new(),
        origin: String::new(),
    }
}

fn check_runner(
    remote_id: i64,
    remote_name: &str,
    remote_set_id: i64,
    expected_id: i64,
) -> (Result<(), HostError>, ScriptedTransport) {
    let binding = binding();
    let expected = RunnerReference {
        id: expected_id,
        name: "runner-a".to_owned(),
        runner_scale_set_id: 17,
    };
    let mut transport = transport(remote_id, remote_name, remote_set_id);
    let result = audit_remote_runner_with_transport(
        &binding,
        &expected,
        17,
        "pat-value-not-recorded",
        &mut transport,
    );
    (result, transport)
}

#[test]
fn remote_runner_audit_requires_exact_repository_runner_and_scale_set() {
    let (result, transport) = check_runner(42, "runner-a", 17, 42);
    assert!(result.is_ok());
    assert_eq!(
        transport.paths,
        [
            "/repos/donbeave/essential-mac",
            "/repos/donbeave/essential-mac/actions/runners/registration-token",
            "/actions/runner-registration",
            "_apis/distributedtask/pools/0/agents",
        ]
    );
    assert_eq!(transport.origin, "https://actions.example.test");

    assert!(check_runner(43, "runner-a", 17, 42).0.is_err());
    assert!(check_runner(42, "runner-a", 18, 42).0.is_err());
    assert!(check_runner(42, "runner-b", 17, 42).0.is_err());
}

#[test]
fn remote_runner_audit_rejects_a_runner_outside_the_polled_scale_set() {
    let binding = binding();
    let expected = RunnerReference {
        id: 42,
        name: "runner-a".to_owned(),
        runner_scale_set_id: 18,
    };
    let mut transport = transport(42, "runner-a", 18);
    assert!(
        audit_remote_runner_with_transport(
            &binding,
            &expected,
            17,
            "pat-value-not-recorded",
            &mut transport,
        )
        .is_err()
    );
    assert_eq!(transport.paths, Vec::<String>::new());
}
