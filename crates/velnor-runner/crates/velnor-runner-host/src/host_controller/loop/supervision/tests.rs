use std::{
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
    time::Duration,
};

use crate::{
    error::HostError,
    journal::{LocalWorkerExitKind, request_lifecycle::tests::helpers},
    launch::{SelectedEngineContext, bootstrap::cleanup_runtime_fixture},
};

use super::{LocalWorkerHealthSupervisor, supervise_local_worker_health};

static NEXT_SOCKET_ID: AtomicU64 = AtomicU64::new(0);

#[test]
fn retiring_binding_records_a_real_exited_runner() -> Result<(), String> {
    crate::launch::test_harness_stack::run_on_test_harness_stack(
        "retiring-binding-worker-health",
        || {
            Box::pin(async {
                retiring_binding_records_a_real_exited_runner_inner()
                    .await
                    .map_err(|error| error.to_string())
            })
        },
    )
}

async fn retiring_binding_records_a_real_exited_runner_inner() -> Result<(), HostError> {
    let fixture = cleanup_runtime_fixture().await?;
    fixture.base.record_started().await?;
    fixture
        .server
        .expose_owned_containers(fixture.scope.connection_id().as_str())
        .await?;
    let binding = helpers::binding("native-started-intake", 101)?;
    let docker = fixture.server.docker();
    let selected = selected_engine(&fixture.scope, &docker).await?;
    let mut supervisor = LocalWorkerHealthSupervisor::default();

    supervise_local_worker_health(
        &mut supervisor,
        &fixture.journal,
        &docker,
        &selected,
        &[],
        std::slice::from_ref(&binding),
    )
    .await;

    let exits = fixture.scope.unresolved_local_worker_exits().await?;
    assert_eq!(exits.len(), 1);
    assert_eq!(
        exits[0].kind(),
        LocalWorkerExitKind::Exited { exit_code: 0 }
    );
    assert_eq!(
        exits[0].runner_container_id(),
        fixture.base.channel.runner_container_id()
    );
    Ok(())
}

#[test]
fn mismatched_inspect_identity_keeps_the_worker_held() -> Result<(), String> {
    crate::launch::test_harness_stack::run_on_test_harness_stack(
        "worker-health-inspect-identity-mismatch",
        || {
            Box::pin(async {
                mismatched_inspect_identity_keeps_the_worker_held_inner()
                    .await
                    .map_err(|error| error.to_string())
            })
        },
    )
}

async fn mismatched_inspect_identity_keeps_the_worker_held_inner() -> Result<(), HostError> {
    let fixture = cleanup_runtime_fixture().await?;
    fixture.base.record_started().await?;
    fixture
        .server
        .expose_owned_containers_with_mismatch(
            fixture.scope.connection_id().as_str(),
            crate::launch::bootstrap::OwnedContainerMismatch::Id,
        )
        .await?;
    let binding = helpers::binding("native-started-intake", 101)?;
    let docker = fixture.server.docker();
    let selected = selected_engine(&fixture.scope, &docker).await?;
    let mut supervisor = LocalWorkerHealthSupervisor::default();

    supervise_local_worker_health(
        &mut supervisor,
        &fixture.journal,
        &docker,
        &selected,
        std::slice::from_ref(&binding),
        &[],
    )
    .await;

    assert_eq!(
        fixture.scope.unresolved_local_worker_exits().await?,
        Vec::new()
    );
    assert_eq!(fixture.journal.occupied_reservation_count().await?, 1);
    Ok(())
}

#[test]
fn running_runner_from_docker_http_stays_unquarantined() -> Result<(), String> {
    crate::launch::test_harness_stack::run_on_test_harness_stack(
        "worker-health-running-runner",
        || {
            Box::pin(async {
                running_runner_from_docker_http_stays_unquarantined_inner()
                    .await
                    .map_err(|error| error.to_string())
            })
        },
    )
}

async fn running_runner_from_docker_http_stays_unquarantined_inner() -> Result<(), HostError> {
    let fixture = cleanup_runtime_fixture().await?;
    fixture.base.record_started().await?;
    let binding = helpers::binding("native-started-intake", 101)?;
    let candidates = all_candidates(&fixture.scope).await?;
    assert_eq!(candidates.len(), 1);
    let docker = fixture.server.docker();
    let selected = selected_engine(&fixture.scope, &docker).await?;
    let info = docker.info().await.map_err(|_| HostError::Docker)?;
    let (running_docker, _server) = RunningInspectServer::open(Duration::ZERO, &info)?;
    let mut supervisor = LocalWorkerHealthSupervisor::default();

    supervise_local_worker_health(
        &mut supervisor,
        &fixture.journal,
        &running_docker,
        &selected,
        std::slice::from_ref(&binding),
        &[],
    )
    .await;

    assert_eq!(
        fixture.scope.unresolved_local_worker_exits().await?,
        Vec::new()
    );
    assert_eq!(all_candidates(&fixture.scope).await?.len(), 1);
    assert_eq!(fixture.journal.occupied_reservation_count().await?, 1);
    Ok(())
}

pub(super) async fn selected_engine(
    scope: &crate::journal::ConnectionJournal,
    docker: &bollard::Docker,
) -> Result<SelectedEngineContext, HostError> {
    let engine = scope.engine_context().await?.ok_or(HostError::Ownership)?;
    let info = docker.info().await.map_err(|_| HostError::Docker)?;
    SelectedEngineContext::capture(&engine, &info)
}

pub(super) async fn all_candidates(
    scope: &crate::journal::ConnectionJournal,
) -> Result<Vec<crate::journal::LocalWorkerHealthCandidate>, HostError> {
    let mut after_reservation_id = 0;
    let mut candidates = Vec::new();
    loop {
        let (mut page, scanned_through, has_more) = scope
            .local_worker_health_candidate_page_after(after_reservation_id)
            .await?;
        candidates.append(&mut page);
        if !has_more {
            return Ok(candidates);
        }
        after_reservation_id = scanned_through.ok_or(HostError::Journal)?;
    }
}

pub(super) struct RunningInspectServer {
    path: PathBuf,
    task: tokio::task::JoinHandle<()>,
}

impl RunningInspectServer {
    fn open(
        delay: Duration,
        info: &bollard::models::SystemInfo,
    ) -> Result<(bollard::Docker, Self), HostError> {
        Self::open_with_behavior(delay, false, false, false, info)
    }

    pub(super) fn open_first_slow_then_exited(
        delay: Duration,
        info: &bollard::models::SystemInfo,
    ) -> Result<(bollard::Docker, Self), HostError> {
        Self::open_with_behavior(delay, true, false, false, info)
    }

    fn open_missing(
        info: &bollard::models::SystemInfo,
    ) -> Result<(bollard::Docker, Self), HostError> {
        Self::open_with_behavior(Duration::ZERO, false, true, false, info)
    }

    fn open_missing_with_info_error(
        info: &bollard::models::SystemInfo,
    ) -> Result<(bollard::Docker, Self), HostError> {
        Self::open_with_behavior(Duration::ZERO, false, true, true, info)
    }

    fn open_with_behavior(
        delay: Duration,
        exit_after_first: bool,
        missing: bool,
        info_error: bool,
        info: &bollard::models::SystemInfo,
    ) -> Result<(bollard::Docker, Self), HostError> {
        use tokio::net::UnixListener;

        let info = serde_json::to_string(info).map_err(|_| HostError::Journal)?;
        let path = std::env::temp_dir().join(format!(
            "velnor-worker-health-running-{}-{}.sock",
            std::process::id(),
            NEXT_SOCKET_ID.fetch_add(1, Ordering::Relaxed)
        ));
        let listener = UnixListener::bind(&path).map_err(|_| HostError::Path)?;
        let behavior = InspectServerBehavior {
            delay,
            exit_after_first,
            missing,
            info_error,
            info,
        };
        let task = tokio::spawn(serve_inspect_server(listener, behavior));
        let socket = path.to_str().ok_or(HostError::Path)?;
        let docker = bollard::Docker::connect_with_unix(socket, 120, bollard::API_DEFAULT_VERSION)
            .map_err(|_| HostError::Docker)?;
        Ok((docker, Self { path, task }))
    }
}

struct InspectServerBehavior {
    delay: Duration,
    exit_after_first: bool,
    missing: bool,
    info_error: bool,
    info: String,
}

async fn serve_inspect_server(listener: tokio::net::UnixListener, behavior: InspectServerBehavior) {
    let mut inspect_count = 0;
    loop {
        let Ok((stream, _)) = listener.accept().await else {
            return;
        };
        serve_inspect_connection(stream, &behavior, &mut inspect_count).await;
    }
}

async fn serve_inspect_connection(
    mut stream: tokio::net::UnixStream,
    behavior: &InspectServerBehavior,
    inspect_count: &mut usize,
) {
    let request = read_inspect_request(&mut stream).await;
    let request_path = request
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .unwrap_or_default();
    if request_path.ends_with("/info") {
        let (status, body) = if behavior.info_error {
            (
                "500 Internal Server Error",
                "{\"message\":\"unavailable\"}".to_owned(),
            )
        } else {
            ("200 OK", behavior.info.clone())
        };
        write_inspect_response(&mut stream, status, &body).await;
    } else if behavior.missing {
        write_inspect_response(&mut stream, "404 Not Found", "{\"message\":\"not found\"}").await;
    } else {
        serve_runner_inspect(stream, request_path, behavior, inspect_count).await;
    }
}

async fn read_inspect_request(stream: &mut tokio::net::UnixStream) -> String {
    use tokio::io::AsyncReadExt;

    let mut request = Vec::new();
    loop {
        let mut bytes = [0; 2048];
        let Ok(count) = stream.read(&mut bytes).await else {
            break;
        };
        if count == 0 {
            break;
        }
        request.extend_from_slice(&bytes[..count]);
        if request.windows(4).any(|window| window == b"\r\n\r\n") {
            break;
        }
    }
    String::from_utf8_lossy(&request).into_owned()
}

async fn serve_runner_inspect(
    mut stream: tokio::net::UnixStream,
    request_path: &str,
    behavior: &InspectServerBehavior,
    inspect_count: &mut usize,
) {
    use tokio::io::AsyncReadExt;

    let id = request_path
        .split("/containers/")
        .nth(1)
        .and_then(|path| path.split("/json").next())
        .unwrap_or_default();
    let delayed_first = behavior.exit_after_first && *inspect_count == 0;
    *inspect_count += 1;
    if delayed_first {
        let mut closed = [0; 1];
        let cancelled = tokio::select! {
            () = tokio::time::sleep(behavior.delay) => false,
            _ = stream.read(&mut closed) => true,
        };
        if cancelled {
            return;
        }
    } else if !behavior.exit_after_first {
        tokio::time::sleep(behavior.delay).await;
    }
    let state = if behavior.exit_after_first && !delayed_first {
        serde_json::json!({"Status": "exited", "Running": false, "ExitCode": 23})
    } else {
        serde_json::json!({"Status": "running", "Running": true})
    };
    let body = serde_json::json!({"Id": id, "State": state}).to_string();
    write_inspect_response(&mut stream, "200 OK", &body).await;
}

async fn write_inspect_response(stream: &mut tokio::net::UnixStream, status: &str, body: &str) {
    use tokio::io::AsyncWriteExt;

    let response = format!(
        "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len(),
    );
    let _sent = stream.write_all(response.as_bytes()).await;
}

impl Drop for RunningInspectServer {
    fn drop(&mut self) {
        self.task.abort();
        let _removed = std::fs::remove_file(&self.path).err();
    }
}

#[path = "tests/engine_identity_tests.rs"]
mod engine_identity_tests;
