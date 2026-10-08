use std::io::Write;
use std::net::{TcpListener, TcpStream};
use std::thread;
use std::time::{Duration, Instant};

use velnor_runner_github::{
    ActionsJobReconciliationState, DiscoveryTransport, Method, ObservedScaleSetJob, Transport,
    reconcile_observed_scale_set_job_async,
};

use super::{
    BoundedDiscoveryTransport, Origin, actions_get, actions_rest_get, api_get,
    validate_discovery_request,
};

#[test]
fn only_exact_workflow_run_and_attempt_jobs_routes_are_allowlisted() {
    let api = Origin::GithubApi;
    for path in [
        "repos/acme/widget/actions/runs/1",
        "repos/ChainArgos/java-monorepo/actions/runs/88",
        "repos/acme/widget/actions/runs/18446744073709551615",
    ] {
        assert!(
            validate_discovery_request(&api, &actions_rest_get(path)).is_some(),
            "{path}"
        );
    }

    for page in 1..=4 {
        let mut request =
            actions_rest_get("repos/ChainArgos/java-monorepo/actions/runs/88/attempts/2/jobs");
        request.query = Some(format!("per_page=100&page={page}"));
        assert!(
            validate_discovery_request(&api, &request).is_some(),
            "page {page}"
        );
    }
}

#[test]
fn invalid_actions_paths_queries_methods_headers_and_origins_stay_rejected() {
    let api = Origin::GithubApi;
    let actions =
        Origin::Actions("https://pipelinesghubeus13.actions.githubusercontent.com".to_owned());
    for path in [
        "repos/acme/widget/actions/runs/0",
        "repos/acme/widget/actions/runs/01",
        "repos/acme/widget/actions/runs/+1",
        "repos/acme/widget/actions/runs/18446744073709551616",
        "repos/acme/widget/actions/runs/88/jobs",
        "repos/acme/widget/actions/runs/88/attempts/1",
        "repos/acme/widget/actions/runs/88/attempts/0/jobs",
        "repos/acme/widget/actions/runs/88/attempts/01/jobs",
        "repos/acme/widget/actions/runs/88/attempts/18446744073709551616/jobs",
        "repos/acme/widget/actions/runs/88/attempts/1/jobs/extra",
        "repos/acme/widget/actions/runs/88/attempts/1/jobs/",
        "repos/acme/widget/actions/runs/88/attempts/1/jobs",
        "repos/acme/widget/actions/runs//attempts/1/jobs",
        "orgs/acme/actions/runs/88",
    ] {
        assert!(
            validate_discovery_request(&api, &actions_rest_get(path)).is_none(),
            "unexpectedly accepted {path}"
        );
    }

    for query in [
        "per_page=100&page=0",
        "per_page=100&page=01",
        "per_page=100&page=5",
        "per_page=100&page=255",
        "per_page=100&page=1&x=y",
        "per_page=100&page=1&page=2",
        "page=1&per_page=100",
        "per_page=50&page=1",
        "per_page=100&page=%31",
        "per_page=100&page=",
    ] {
        let mut request = actions_rest_get("repos/acme/widget/actions/runs/88/attempts/1/jobs");
        request.query = Some(query.to_owned());
        assert!(
            validate_discovery_request(&api, &request).is_none(),
            "unexpectedly accepted query {query}"
        );
    }

    let mut run_query = actions_rest_get("repos/acme/widget/actions/runs/88");
    run_query.query = Some("per_page=100&page=1".to_owned());
    assert!(validate_discovery_request(&api, &run_query).is_none());

    let mut repository_query = api_get("repos/acme/widget");
    repository_query.query = Some("page=1".to_owned());
    assert!(validate_discovery_request(&api, &repository_query).is_none());

    let mut post = actions_rest_get("repos/acme/widget/actions/runs/88");
    post.method = Method::Post;
    assert!(validate_discovery_request(&api, &post).is_none());

    let mut body = actions_rest_get("repos/acme/widget/actions/runs/88");
    body.body = b"{}".to_vec();
    assert!(validate_discovery_request(&api, &body).is_none());

    let mut bad_headers = actions_rest_get("repos/acme/widget/actions/runs/88");
    bad_headers.headers[0].1 = "application/json".to_owned();
    assert!(validate_discovery_request(&api, &bad_headers).is_none());
    bad_headers.headers[0].1 = "application/vnd.github+json".to_owned();
    bad_headers
        .headers
        .push(("Accept".to_owned(), "application/json".to_owned()));
    assert!(validate_discovery_request(&api, &bad_headers).is_none());

    assert!(
        validate_discovery_request(
            &actions,
            &actions_rest_get("repos/acme/widget/actions/runs/88")
        )
        .is_none()
    );
    assert!(
        validate_discovery_request(
            &api,
            &actions_get("_apis/runtime/runnergroups", "api-version=6.0-preview")
        )
        .is_none()
    );
}

#[test]
fn exact_actions_read_requests_dispatch_through_bounded_transport() {
    for (path, query, expected_target) in [
        (
            "repos/ChainArgos/java-monorepo/actions/runs/88",
            None,
            "/repos/ChainArgos/java-monorepo/actions/runs/88",
        ),
        (
            "repos/ChainArgos/java-monorepo/actions/runs/88/attempts/2/jobs",
            Some("per_page=100&page=3"),
            "/repos/ChainArgos/java-monorepo/actions/runs/88/attempts/2/jobs?per_page=100&page=3",
        ),
    ] {
        let (base, server) = super::support::response_server(b"{}");
        let mut transport = BoundedDiscoveryTransport::for_test(base, 4096, Duration::from_secs(2));
        transport
            .bind_github_api_origin()
            .expect("fixed GitHub API origin should bind");
        let mut request = actions_rest_get(path);
        request.query = query.map(str::to_owned);
        let exchange = transport
            .exchange(&request)
            .expect("exact read-only Actions route should dispatch");
        assert_eq!(exchange.status, 200);
        let observed = server.join().expect("fake server should finish");
        assert_eq!(observed.route, expected_target);
        assert_eq!(observed.authorization_scheme, "Bearer");
        assert!(observed.credential_matches);
    }
}

#[tokio::test]
async fn github_reconciliation_helper_dispatches_its_exact_actions_read_routes() {
    let (base, server) = reconciliation_server();
    let mut transport = BoundedDiscoveryTransport::for_test(base, 4096, Duration::from_secs(3));
    let result = reconcile_observed_scale_set_job_async(
        &mut transport,
        "ChainArgos",
        "java-monorepo",
        ObservedScaleSetJob {
            scale_set_job_id: Some("opaque-scale-job-119"),
            workflow_run_id: Some(88),
            runner_id: Some(31),
            runner_name: Some("velnor-job-31"),
        },
        "actions-read-canary",
    )
    .await
    .expect("actual helper requests should pass the bounded transport");

    assert_eq!(result.state, ActionsJobReconciliationState::Completed);
    assert_eq!(
        server.join().expect("fake GitHub server should finish"),
        [
            RouteObservation {
                target: "/repos/ChainArgos/java-monorepo/actions/runs/88".to_owned(),
                bearer_matches: true,
            },
            RouteObservation {
                target: "/repos/ChainArgos/java-monorepo/actions/runs/88/attempts/1/jobs?per_page=100&page=1".to_owned(),
                bearer_matches: true,
            },
        ]
    );
}

#[derive(Debug, PartialEq, Eq)]
struct RouteObservation {
    target: String,
    bearer_matches: bool,
}

fn reconciliation_server() -> (String, thread::JoinHandle<Vec<RouteObservation>>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("fake GitHub listener should bind");
    listener
        .set_nonblocking(true)
        .expect("fake GitHub listener should be nonblocking");
    let base = format!(
        "http://{}",
        listener.local_addr().expect("listener address")
    );
    let responses: [&[u8]; 2] = [
        br#"{"id":88,"path":".github/workflows/ci.yml","run_attempt":1,"status":"completed","conclusion":"success","event":"push","head_sha":"abc123","head_repository":{"full_name":"ChainArgos/java-monorepo"}}"#,
        br#"{"total_count":1,"jobs":[{"id":119,"run_id":88,"status":"completed","conclusion":"success","runner_id":31,"runner_name":"velnor-job-31","runner_group_id":19,"runner_group_name":"Velnor"}]}"#,
    ];
    let server = thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut observations = Vec::with_capacity(responses.len());
        for body in responses {
            let (mut stream, observation) = accept_request(&listener, deadline);
            let response = super::support::http_response(200, body);
            stream
                .write_all(&response)
                .expect("fake GitHub response should write");
            observations.push(observation);
        }
        observations
    });
    (base, server)
}

fn accept_request(listener: &TcpListener, deadline: Instant) -> (TcpStream, RouteObservation) {
    let (mut stream, _) = loop {
        match listener.accept() {
            Ok(connection) => break connection,
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                assert!(Instant::now() < deadline, "fake GitHub deadline elapsed");
                thread::sleep(Duration::from_millis(5));
            }
            Err(error) => panic!("fake GitHub accept failed: {error}"),
        }
    };
    let headers = super::support::read_headers(&mut stream);
    let text = std::str::from_utf8(&headers).expect("request headers should be ASCII");
    let first_line = text.lines().next().expect("request line should exist");
    let mut request_line = first_line.split_whitespace();
    assert_eq!(request_line.next(), Some("GET"));
    let target = request_line
        .next()
        .expect("request target should exist")
        .to_owned();
    let bearer_matches = text.lines().any(|line| {
        line.split_once(':').is_some_and(|(name, value)| {
            name.eq_ignore_ascii_case("authorization")
                && value.trim() == "Bearer actions-read-canary"
        })
    });
    (
        stream,
        RouteObservation {
            target,
            bearer_matches,
        },
    )
}
