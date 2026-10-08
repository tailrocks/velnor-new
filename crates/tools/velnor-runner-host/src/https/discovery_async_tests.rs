use std::fs;
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use velnor_runner_github::{
    AsyncDiscoveryIntentStore, AsyncDiscoveryTransport, BearerRole, DiscoveryCredentialOutcome,
    DiscoveryCredentialStep, DiscoveryIntentId, DiscoveryStoreFuture, Method, RequestPurpose,
    ScaleSetFound, SessionRequest, TransportFail, exchange_repository_discovery_admin_once_async,
    issue_repository_discovery_token_async, read_repository_admin_evidence_async,
};

use super::BoundedDiscoveryTransport;
use super::flow::{assert_successful_requests, successful_discovery_server};
use super::support::{TestDirectory, process_exists, response_server, write_stub};

#[test]
fn async_transport_runs_the_repository_read_on_the_bounded_http_worker() {
    let body =
        br#"{"id":123,"full_name":"acme/widget","private":true,"permissions":{"admin":true}}"#;
    let (base, server) = response_server(body);
    let mut transport = BoundedDiscoveryTransport::for_test(base, 4096, Duration::from_secs(3));
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("test runtime should build");

    let evidence = runtime
        .block_on(read_repository_admin_evidence_async(
            &mut transport,
            "acme",
            "widget",
            "host-secret",
        ))
        .expect("repository facts should decode");
    assert_eq!(evidence.repository_id(), 123);

    let observed = server.join().expect("server should finish");
    assert_eq!(observed.route, "/repos/acme/widget");
    assert_eq!(observed.authorization_scheme, "Bearer");
    assert!(observed.credential_matches);
}

#[test]
fn full_async_discovery_uses_the_bounded_worker_and_exact_route_credentials() {
    let (base, server) = successful_discovery_server();
    let mut transport =
        BoundedDiscoveryTransport::for_test(base, 32 * 1024, Duration::from_secs(3));
    let mut intents = AsyncRecordedIntents::default();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("test runtime should build");

    let (groups, scale_set) = runtime.block_on(async {
        let evidence =
            read_repository_admin_evidence_async(&mut transport, "acme", "widget", "host-secret")
                .await
                .expect("repository admin evidence should be parsed");
        assert_eq!(evidence.repository_id(), 22);
        let registration = issue_repository_discovery_token_async(
            &mut transport,
            evidence,
            "host-secret",
            &mut intents,
        )
        .await
        .expect("one repository token should be issued");
        let admin = exchange_repository_discovery_admin_once_async(
            &mut transport,
            registration,
            &mut intents,
        )
        .await
        .expect("one Actions discovery credential should be issued");
        let groups = admin
            .list_runner_groups_async(&mut transport)
            .await
            .expect("runner groups should be readable");
        let scale_set = admin
            .get_existing_product_scale_set_async(&mut transport, 19, "ubuntu-24.04-scale-set")
            .await
            .expect("existing Scale Set lookup should remain read-only");
        (groups, scale_set)
    });

    assert_eq!(groups.len(), 1);
    assert_eq!(groups[0].id, 19);
    assert_eq!(groups[0].name, "Velnor");
    assert_eq!(scale_set, ScaleSetFound::NotFound);
    assert_eq!(
        intents.events,
        [
            AsyncIntentEvent::Before(
                DiscoveryCredentialStep::RepositoryRegistrationToken,
                22,
                "acme/widget".to_owned(),
            ),
            AsyncIntentEvent::Outcome(1, DiscoveryCredentialOutcome::Succeeded),
            AsyncIntentEvent::Before(
                DiscoveryCredentialStep::ActionsAdminExchange,
                22,
                "acme/widget".to_owned(),
            ),
            AsyncIntentEvent::Outcome(2, DiscoveryCredentialOutcome::Succeeded),
        ]
    );
    let requests = server.join().expect("test server should finish");
    assert_successful_requests(&requests);
}

#[test]
fn async_transport_deadline_starts_before_the_returned_future_is_polled() {
    let directory = TestDirectory::new();
    let executable = directory.path.join("curl-stub");
    let marker = directory.path.join("spawned");
    write_stub(
        &executable,
        &format!("#!/bin/sh\n: > '{}'\n", marker.display()),
    );
    let mut transport = BoundedDiscoveryTransport::for_test(
        "http://127.0.0.1:1".to_owned(),
        1024,
        Duration::from_millis(25),
    );
    transport.curl = executable.to_string_lossy().into_owned();
    transport
        .bind_github_api_origin()
        .expect("fixed API origin should bind");
    let exchange = transport.exchange_discovery(repository_get());
    thread::sleep(Duration::from_millis(60));

    let result = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("test runtime should build")
        .block_on(exchange);
    assert_eq!(result, Err(TransportFail::Timeout));
    assert!(!marker.exists(), "expired work must not spawn curl");
}

#[test]
fn async_deadline_returns_only_after_the_owned_curl_worker_is_reaped() {
    let directory = TestDirectory::new();
    let executable = directory.path.join("curl-stub");
    let pid_file = directory.path.join("pid");
    write_stub(
        &executable,
        &format!(
            "#!/bin/sh\nprintf '%s' \"$$\" > '{}'\nexec sleep 30\n",
            pid_file.display()
        ),
    );
    let mut transport = BoundedDiscoveryTransport::for_test(
        "http://127.0.0.1:1".to_owned(),
        1024,
        Duration::from_millis(150),
    );
    transport.curl = executable.to_string_lossy().into_owned();
    transport
        .bind_github_api_origin()
        .expect("fixed API origin should bind");
    let exchange = transport.exchange_discovery(repository_get());
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("test runtime should build");

    let started = Instant::now();
    assert_eq!(
        runtime.block_on(exchange),
        Err(TransportFail::Timeout),
        "an expired request must report timeout"
    );
    assert!(started.elapsed() < Duration::from_secs(2));

    let pid = fs::read_to_string(pid_file).expect("stub PID should be recorded");
    assert!(
        !process_exists(pid.trim()),
        "timeout must not return while its curl child is still alive"
    );
}

#[test]
fn async_deadline_is_not_queued_behind_tokios_blocking_pool() {
    let directory = TestDirectory::new();
    let executable = directory.path.join("curl-stub");
    let pid_file = directory.path.join("pid");
    write_stub(
        &executable,
        &format!(
            "#!/bin/sh\nprintf '%s' \"$$\" > '{}'\nexec sleep 30\n",
            pid_file.display()
        ),
    );
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(1)
        .max_blocking_threads(1)
        .enable_all()
        .build()
        .expect("test runtime should build");
    let (started_sender, started_receiver) = mpsc::sync_channel(1);
    let blocker = runtime.spawn_blocking(move || {
        started_sender
            .send(())
            .expect("blocking-pool test signal should send");
        thread::sleep(Duration::from_millis(500));
    });
    started_receiver
        .recv_timeout(Duration::from_secs(2))
        .expect("blocking thread should occupy the sole Tokio blocking slot");

    let mut transport = BoundedDiscoveryTransport::for_test(
        "http://127.0.0.1:1".to_owned(),
        1024,
        Duration::from_millis(150),
    );
    transport.curl = executable.to_string_lossy().into_owned();
    transport
        .bind_github_api_origin()
        .expect("fixed API origin should bind");
    let started = Instant::now();
    assert_eq!(
        runtime.block_on(transport.exchange_discovery(repository_get())),
        Err(TransportFail::Timeout),
        "the absolute request deadline should still apply"
    );
    assert!(
        started.elapsed() < Duration::from_millis(400),
        "bounded transport must not wait for an unrelated Tokio blocking-pool job"
    );
    let pid = fs::read_to_string(pid_file).expect("stub PID should be recorded");
    assert!(
        !process_exists(pid.trim()),
        "the timeout result must follow curl child reaping"
    );
    runtime
        .block_on(blocker)
        .expect("test blocking-pool task should finish");
}

#[test]
fn dropping_the_async_exchange_cancels_and_reaps_the_owned_curl_worker() {
    let directory = TestDirectory::new();
    let executable = directory.path.join("curl-stub");
    let pid_file = directory.path.join("pid");
    write_stub(
        &executable,
        &format!(
            "#!/bin/sh\nprintf '%s' \"$$\" > '{}'\ncat >/dev/null\nexec sleep 30\n",
            pid_file.display()
        ),
    );
    let mut transport = BoundedDiscoveryTransport::for_test(
        "http://127.0.0.1:1".to_owned(),
        1024,
        Duration::from_secs(5),
    );
    transport.curl = executable.to_string_lossy().into_owned();
    transport
        .bind_github_api_origin()
        .expect("fixed API origin should bind");
    let exchange = transport.exchange_discovery(repository_get());
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("test runtime should build");

    runtime.block_on(async {
        let task = tokio::spawn(exchange);
        tokio::task::yield_now().await;
        let started = Instant::now();
        while !pid_file.exists() && started.elapsed() < Duration::from_secs(2) {
            thread::sleep(Duration::from_millis(5));
        }
        let process_started = pid_file.exists();
        task.abort();
        let _cancelled = task.await;
        assert!(process_started, "the bounded worker should have started");
    });

    let pid = fs::read_to_string(&pid_file).expect("stub PID should be recorded");
    let stopped_at = Instant::now();
    while process_exists(pid.trim()) && stopped_at.elapsed() < Duration::from_secs(2) {
        thread::sleep(Duration::from_millis(5));
    }
    assert!(
        !process_exists(pid.trim()),
        "cancellation must kill and reap curl before the worker finishes"
    );
    assert!(stopped_at.elapsed() < Duration::from_secs(2));
}

fn repository_get() -> SessionRequest {
    SessionRequest {
        purpose: RequestPurpose::RepositoryRead,
        bearer_role: BearerRole::GithubRestCredential,
        method: Method::Get,
        path: "repos/acme/widget".to_owned(),
        query: None,
        headers: vec![
            (
                "Accept".to_owned(),
                "application/vnd.github+json".to_owned(),
            ),
            ("Authorization".to_owned(), "Bearer host-secret".to_owned()),
            ("X-GitHub-Api-Version".to_owned(), "2026-03-10".to_owned()),
            ("User-Agent".to_owned(), "velnor-host".to_owned()),
        ],
        body: Vec::new(),
    }
}

#[derive(Debug, PartialEq, Eq)]
enum AsyncIntentEvent {
    Before(DiscoveryCredentialStep, i64, String),
    Outcome(u64, DiscoveryCredentialOutcome),
}

#[derive(Default)]
struct AsyncRecordedIntents {
    next_id: u64,
    events: Vec<AsyncIntentEvent>,
}

impl AsyncDiscoveryIntentStore for AsyncRecordedIntents {
    fn persist_before<'a>(
        &'a mut self,
        step: DiscoveryCredentialStep,
        repository_id: i64,
        full_name: &'a str,
    ) -> DiscoveryStoreFuture<'a, DiscoveryIntentId> {
        self.next_id += 1;
        let id = DiscoveryIntentId::new(self.next_id).expect("positive intent id");
        self.events.push(AsyncIntentEvent::Before(
            step,
            repository_id,
            full_name.to_owned(),
        ));
        Box::pin(async move { Ok(id) })
    }

    fn record_outcome(
        &mut self,
        id: DiscoveryIntentId,
        outcome: DiscoveryCredentialOutcome,
    ) -> DiscoveryStoreFuture<'_, ()> {
        self.events
            .push(AsyncIntentEvent::Outcome(id.get(), outcome));
        Box::pin(async { Ok(()) })
    }
}
