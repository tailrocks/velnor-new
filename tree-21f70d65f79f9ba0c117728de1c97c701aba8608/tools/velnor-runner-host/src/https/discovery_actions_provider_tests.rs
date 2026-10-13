use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::thread;
use std::time::{Duration, Instant};

use velnor_runner_github::{
    ActionsWorkflowAttemptProviderRead, BearerRole, DiscoveryTransport, Method, RequestPurpose,
    SessionRequest, Transport, read_actions_workflow_attempt_provider_evidence_async,
};

use super::BoundedDiscoveryTransport;

const HEAD_SHA: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const ACTIONS_TOKEN: &str = "provider-read-canary";

#[tokio::test]
async fn provider_reader_uses_bounded_host_transport_for_exact_routes() {
    let (base, server) = provider_server(vec![attempt_json(), jobs_json(1), artifacts_json()]);
    let mut transport = BoundedDiscoveryTransport::for_test(
        base,
        super::MAX_RESPONSE_BYTES,
        super::REQUEST_DEADLINE,
    );
    let result = read_actions_workflow_attempt_provider_evidence_async(
        &mut transport,
        "ChainArgos",
        "java-monorepo",
        88,
        2,
        HEAD_SHA,
        ACTIONS_TOKEN,
    )
    .await
    .expect("provider helper should complete through bounded transport");

    let ActionsWorkflowAttemptProviderRead::Complete(evidence) = result else {
        panic!("the exact small fixture should be complete");
    };
    assert_eq!(evidence.workflow_run_id, 88);
    assert_eq!(evidence.attempt, 2);
    assert_eq!(evidence.repository_id, 829_618_808);
    assert_eq!(evidence.head_sha, HEAD_SHA);
    assert_eq!(evidence.jobs.len(), 1);
    assert_eq!(evidence.jobs[0].runner_id, None);
    assert_eq!(evidence.jobs[0].runner_name, None);
    assert_eq!(evidence.jobs[0].runner_group_id, None);
    assert_eq!(evidence.jobs[0].runner_group_name, None);
    assert_eq!(evidence.artifacts.len(), 0);

    assert_eq!(
        server.join().expect("fake API server should finish"),
        [
            ProviderObservation::expected(
                "/repos/ChainArgos/java-monorepo/actions/runs/88/attempts/2",
            ),
            ProviderObservation::expected(
                "/repos/ChainArgos/java-monorepo/actions/runs/88/attempts/2/jobs?per_page=100&page=1",
            ),
            ProviderObservation::expected(
                "/repos/ChainArgos/java-monorepo/actions/runs/88/artifacts?per_page=100&page=1",
            ),
        ]
    );
}

#[tokio::test]
async fn provider_reader_returns_limit_without_partial_rows_or_page_five() {
    let (base, server) = provider_server(vec![attempt_json(), jobs_json(401)]);
    let mut transport = BoundedDiscoveryTransport::for_test(
        base,
        super::MAX_RESPONSE_BYTES,
        super::REQUEST_DEADLINE,
    );
    let result = read_actions_workflow_attempt_provider_evidence_async(
        &mut transport,
        "ChainArgos",
        "java-monorepo",
        88,
        2,
        HEAD_SHA,
        ACTIONS_TOKEN,
    )
    .await
    .expect("bounded provider limit is an explicit outcome");

    assert!(matches!(
        result,
        ActionsWorkflowAttemptProviderRead::Unavailable(
            velnor_runner_github::ActionsWorkflowAttemptEvidenceGap::JobPageLimitExceeded
        )
    ));
    assert_eq!(
        server.join().expect("fake API server should finish"),
        [
            ProviderObservation::expected(
                "/repos/ChainArgos/java-monorepo/actions/runs/88/attempts/2",
            ),
            ProviderObservation::expected(
                "/repos/ChainArgos/java-monorepo/actions/runs/88/attempts/2/jobs?per_page=100&page=1",
            ),
        ]
    );
}

#[test]
fn bounded_host_transport_rejects_provider_page_five_before_connecting() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("fake listener should bind");
    listener
        .set_nonblocking(true)
        .expect("listener should be nonblocking");
    let base = format!(
        "http://{}",
        listener
            .local_addr()
            .expect("listener address should resolve")
    );
    let mut transport = BoundedDiscoveryTransport::for_test(
        base,
        super::MAX_RESPONSE_BYTES,
        Duration::from_millis(250),
    );
    transport
        .bind_github_api_origin()
        .expect("fixed GitHub API origin should bind");
    let mut request =
        actions_request("repos/ChainArgos/java-monorepo/actions/runs/88/attempts/2/jobs");
    request.query = Some("per_page=100&page=5".to_owned());
    assert!(transport.exchange(&request).is_err());

    let deadline = Instant::now() + Duration::from_millis(100);
    loop {
        match listener.accept() {
            Ok(_) => panic!("invalid page five reached the local API socket"),
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                if Instant::now() >= deadline {
                    break;
                }
                thread::sleep(Duration::from_millis(5));
            }
            Err(error) => panic!("listener accept failed: {error}"),
        }
    }
}

fn actions_request(path: &str) -> SessionRequest {
    SessionRequest {
        purpose: RequestPurpose::ActionsRead,
        bearer_role: BearerRole::GithubRestCredential,
        method: Method::Get,
        path: path.to_owned(),
        query: None,
        headers: vec![
            (
                "Accept".to_owned(),
                "application/vnd.github+json".to_owned(),
            ),
            (
                "Authorization".to_owned(),
                format!("Bearer {ACTIONS_TOKEN}"),
            ),
            ("X-GitHub-Api-Version".to_owned(), "2026-03-10".to_owned()),
            ("User-Agent".to_owned(), "velnor-host".to_owned()),
        ],
        body: Vec::new(),
    }
}

#[derive(Debug, PartialEq, Eq)]
struct ProviderObservation {
    method: String,
    target: String,
    accept: Option<String>,
    api_version: Option<String>,
    user_agent: Option<String>,
    authorization_matches: bool,
    body_empty: bool,
}

impl ProviderObservation {
    fn expected(target: &str) -> Self {
        Self {
            method: "GET".to_owned(),
            target: target.to_owned(),
            accept: Some("application/vnd.github+json".to_owned()),
            api_version: Some("2026-03-10".to_owned()),
            user_agent: Some("velnor-host".to_owned()),
            authorization_matches: true,
            body_empty: true,
        }
    }
}

fn provider_server(
    responses: Vec<Vec<u8>>,
) -> (String, thread::JoinHandle<Vec<ProviderObservation>>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("fake GitHub listener should bind");
    listener
        .set_nonblocking(true)
        .expect("fake listener should be nonblocking");
    let base = format!(
        "http://{}",
        listener
            .local_addr()
            .expect("listener address should resolve")
    );
    let server = thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(5);
        responses
            .into_iter()
            .map(|body| {
                let (mut stream, observation) = accept_provider_request(&listener, deadline);
                stream
                    .write_all(&http_response(200, &body))
                    .expect("fake API response should write");
                observation
            })
            .collect()
    });
    (base, server)
}

fn accept_provider_request(
    listener: &TcpListener,
    deadline: Instant,
) -> (TcpStream, ProviderObservation) {
    let (mut stream, _) = loop {
        match listener.accept() {
            Ok(connection) => break connection,
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                assert!(Instant::now() < deadline, "fake API deadline elapsed");
                thread::sleep(Duration::from_millis(5));
            }
            Err(error) => panic!("fake API accept failed: {error}"),
        }
    };
    let headers = read_headers(&mut stream);
    let text = std::str::from_utf8(&headers).expect("HTTP request should be ASCII");
    let mut request_line = text
        .lines()
        .next()
        .expect("HTTP request line should exist")
        .split_whitespace();
    let method = request_line.next().unwrap_or_default().to_owned();
    let target = request_line.next().unwrap_or_default().to_owned();
    let observation = ProviderObservation {
        method,
        target,
        accept: header_value(text, "accept"),
        api_version: header_value(text, "x-github-api-version"),
        user_agent: header_value(text, "user-agent"),
        authorization_matches: header_value(text, "authorization")
            .is_some_and(|value| value == format!("Bearer {ACTIONS_TOKEN}")),
        body_empty: request_body_is_empty(&mut stream, text),
    };
    (stream, observation)
}

fn header_value(headers: &str, selected: &str) -> Option<String> {
    let mut values = headers.lines().filter_map(|line| {
        let (name, value) = line.split_once(':')?;
        name.eq_ignore_ascii_case(selected)
            .then(|| value.trim().to_owned())
    });
    let value = values.next()?;
    values.next().is_none().then_some(value)
}

fn request_body_is_empty(stream: &mut TcpStream, headers: &str) -> bool {
    let content_length = headers.lines().find_map(|line| {
        let (name, value) = line.split_once(':')?;
        name.eq_ignore_ascii_case("content-length")
            .then(|| value.trim().parse::<usize>().ok())
            .flatten()
    });
    if content_length.is_some_and(|length| length != 0)
        || headers.lines().any(|line| {
            line.split_once(':').is_some_and(|(name, value)| {
                name.eq_ignore_ascii_case("transfer-encoding")
                    && !value.trim().eq_ignore_ascii_case("identity")
            })
        })
    {
        return false;
    }
    stream
        .set_read_timeout(Some(Duration::from_millis(50)))
        .is_ok_and(|()| {
            let mut byte = [0_u8; 1];
            matches!(stream.read(&mut byte), Err(error) if error.kind() == std::io::ErrorKind::WouldBlock || error.kind() == std::io::ErrorKind::TimedOut)
        })
}

fn attempt_json() -> Vec<u8> {
    format!(
        r#"{{"id":88,"run_attempt":2,"path":".github/workflows/ci.yml@refs/heads/main","status":"completed","conclusion":"success","event":"push","head_sha":"{HEAD_SHA}","head_branch":"main","repository":{{"id":829618808,"full_name":"ChainArgos/java-monorepo"}},"head_repository":{{"id":829618808,"full_name":"ChainArgos/java-monorepo"}}}}"#
    )
    .into_bytes()
}

fn jobs_json(total_count: usize) -> Vec<u8> {
    format!(
        r#"{{"total_count":{total_count},"jobs":[{{"id":119,"run_id":88,"name":"compile (JDK 21)","head_sha":"{HEAD_SHA}","status":"completed","conclusion":"success","runner_id":0,"runner_name":"","runner_group_id":0,"runner_group_name":"","workflow_name":"CI","head_branch":"main","labels":["self-hosted","linux"]}}]}}"#
    )
    .into_bytes()
}

fn artifacts_json() -> Vec<u8> {
    br#"{"total_count":0,"artifacts":[]}"#.to_vec()
}

fn http_response(status: u16, body: &[u8]) -> Vec<u8> {
    let mut response = format!(
        "HTTP/1.1 {status} OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    )
    .into_bytes();
    response.extend_from_slice(body);
    response
}

fn read_headers(stream: &mut TcpStream) -> Vec<u8> {
    stream
        .set_read_timeout(Some(Duration::from_secs(2)))
        .expect("request timeout should set");
    let mut bytes = Vec::new();
    let mut byte = [0_u8; 1];
    while bytes.len() < 16 * 1024 {
        stream
            .read_exact(&mut byte)
            .expect("request headers should be complete");
        bytes.push(byte[0]);
        if bytes.ends_with(b"\r\n\r\n") {
            return bytes;
        }
    }
    panic!("request headers exceeded test bound");
}
