// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{UnixListener, UnixStream};

use super::active;
use crate::error::HostError;
use crate::journal::{
    CapacityLimits, Journal, OperationalCapacityLimits, ProbePhase, ProbeSeed, ResourceProbePurpose,
};
use crate::launch::resource_probe::lifecycle::Deadline;
use crate::launch::resource_probe::projection::{DockerRoot, ProbeProjection, VerifiedProbeImage};

type TestResult<T> = Result<T, Box<dyn std::error::Error>>;

const ENGINE_ID: &str = "engine-test";
const ROOT_PATH: &str = "/var/lib/docker";
const FIRST_OPERATION: &str = "0123456789abcdef0123456789abcdef";
const SECOND_OPERATION: &str = "fedcba9876543210fedcba9876543210";

struct FakeDaemon {
    engine_id: &'static str,
    architecture: &'static str,
    name_visible_after: usize,
    name_inspections: Arc<AtomicUsize>,
}

fn info_body(engine_id: &str, architecture: &str) -> String {
    format!(
        r#"{{"ID":"{engine_id}","DockerRootDir":"{ROOT_PATH}","OSType":"linux","Architecture":"{architecture}","NCPU":4,"MemTotal":8589934592}}"#
    )
}

async fn respond(stream: &mut UnixStream, status: &str, body: &str) -> TestResult<()> {
    let head = format!(
        "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    stream.write_all(head.as_bytes()).await?;
    stream.write_all(body.as_bytes()).await?;
    stream.shutdown().await?;
    Ok(())
}

async fn serve_one(mut stream: UnixStream, daemon: &FakeDaemon) -> TestResult<()> {
    let mut request = Vec::new();
    let mut chunk = [0_u8; 1024];
    while !request.windows(4).any(|window| window == b"\r\n\r\n") {
        let read = stream.read(&mut chunk).await?;
        if read == 0 {
            return Ok(());
        }
        request.extend_from_slice(&chunk[..read]);
    }
    let text = String::from_utf8_lossy(&request);
    let path = text.split_whitespace().nth(1).unwrap_or_default();
    if path.ends_with("/info") {
        return respond(
            &mut stream,
            "200 OK",
            &info_body(daemon.engine_id, daemon.architecture),
        )
        .await;
    }
    if path.contains("/containers/velnor-resource-probe-") {
        let seen = daemon.name_inspections.fetch_add(1, Ordering::SeqCst);
        if seen >= daemon.name_visible_after {
            let name = path.split('/').rev().nth(1).unwrap_or_default().to_owned();
            let body = format!(r#"{{"Id":"{}","Name":"/{name}"}}"#, "a".repeat(64));
            return respond(&mut stream, "200 OK", &body).await;
        }
        let body = r#"{"message":"No such container: velnor-resource-probe"}"#;
        return respond(&mut stream, "404 Not Found", body).await;
    }
    respond(&mut stream, "404 Not Found", r#"{"message":"unsupported"}"#).await
}

fn start_daemon(
    socket: &Path,
    engine_id: &'static str,
    name_visible_after: usize,
) -> TestResult<Arc<AtomicUsize>> {
    start_daemon_on(socket, engine_id, name_visible_after, "x86_64")
}

fn start_daemon_on(
    socket: &Path,
    engine_id: &'static str,
    name_visible_after: usize,
    architecture: &'static str,
) -> TestResult<Arc<AtomicUsize>> {
    let listener = UnixListener::bind(socket)?;
    let name_inspections = Arc::new(AtomicUsize::new(0));
    let daemon = Arc::new(FakeDaemon {
        engine_id,
        architecture,
        name_visible_after,
        name_inspections: Arc::clone(&name_inspections),
    });
    tokio::spawn(async move {
        while let Ok((stream, _)) = listener.accept().await {
            let daemon = Arc::clone(&daemon);
            tokio::spawn(async move {
                let _served = serve_one(stream, &daemon).await;
            });
        }
    });
    Ok(name_inspections)
}

type Target = (crate::config::RunnerPlatform, &'static str);

const AMD64: Target = (crate::config::RunnerPlatform::LinuxAmd64, "x86_64");
const ARM64: Target = (crate::config::RunnerPlatform::LinuxArm64, "aarch64");

fn projection_and_seed(
    journal_instance: &str,
    operation_id: &str,
    (platform, architecture): Target,
) -> TestResult<(ProbeProjection, ProbeSeed, DockerRoot)> {
    let root = DockerRoot::parse(ROOT_PATH)?;
    let binding = "4".repeat(64);
    let image = VerifiedProbeImage::from_journal(
        format!("sha256:{}", "3".repeat(64)),
        platform,
        architecture.to_owned(),
        "2".repeat(40),
        binding.clone(),
    )?;
    let projection = ProbeProjection::build(
        operation_id.to_owned(),
        journal_instance.to_owned(),
        ENGINE_ID.to_owned(),
        root.path(),
        image,
    )?;
    let seed = ProbeSeed {
        operation_id: operation_id.to_owned(),
        instance_id: journal_instance.to_owned(),
        engine_id: ENGINE_ID.to_owned(),
        docker_root_digest: root.digest().to_owned(),
        source_revision: "2".repeat(40),
        runtime_image_id: projection.image.runtime_id().to_owned(),
        image_binding_digest: binding,
        operation_name: projection.name.clone(),
        projection_digest: projection.projection_digest.clone(),
        purpose: ResourceProbePurpose::Capacity,
        request_sha256: projection.request_sha256.clone(),
    };
    Ok((projection, seed, root))
}

async fn journal_with_create_requested(
    dir: &Path,
    quarantined: bool,
) -> TestResult<(Journal, DockerRoot)> {
    journal_with_create_requested_on(dir, quarantined, AMD64).await
}

async fn journal_with_create_requested_on(
    dir: &Path,
    quarantined: bool,
    target: Target,
) -> TestResult<(Journal, DockerRoot)> {
    let journal = Journal::open(&dir.join("journal.db")).await?;
    journal.bind_engine(ENGINE_ID).await?;
    journal
        .configure_capacity_with_operational_limits(
            CapacityLimits {
                jobs: 1,
                cpu_millicores: 8_000,
                memory_bytes: 32 << 30,
            },
            OperationalCapacityLimits {
                cpu_millicores: 8_000,
                memory_bytes: 32 << 30,
            },
        )
        .await?;
    let instance = journal.instance_id().await?;
    let (_, seed, root) = projection_and_seed(&instance, FIRST_OPERATION, target)?;
    journal.prepare_probe(seed).await?;
    journal
        .transition_probe(
            FIRST_OPERATION,
            ProbePhase::Prepared,
            ProbePhase::CreateRequested,
        )
        .await?;
    if quarantined {
        journal.quarantine_probe(FIRST_OPERATION).await?;
    }
    Ok((journal, root))
}

async fn recover_first(
    journal: &Journal,
    root: &DockerRoot,
    socket: &Path,
    deadline: &Deadline,
) -> TestResult<Result<(), HostError>> {
    let docker = crate::docker_client::connect_unix(&socket.to_string_lossy())?;
    let info = docker.info().await?;
    let row = journal.leased_probe().await?.ok_or(HostError::Journal)?;
    Ok(Box::pin(active(&docker, journal, &info, root, deadline, row)).await)
}

async fn assert_next_probe_is_not_blocked(journal: &Journal) -> TestResult<()> {
    assert!(journal.leased_probe().await?.is_none());
    let instance = journal.instance_id().await?;
    let (_, seed, _) = projection_and_seed(&instance, SECOND_OPERATION, AMD64)?;
    journal.prepare_probe(seed).await?;
    Ok(())
}

async fn timed_out_create_without_container_unblocks(quarantined: bool) -> TestResult<()> {
    let dir = tempfile::tempdir()?;
    let socket = dir.path().join("docker.sock");
    start_daemon(&socket, ENGINE_ID, usize::MAX)?;
    let (journal, root) = journal_with_create_requested(dir.path(), quarantined).await?;
    let outcome = Box::pin(recover_first(&journal, &root, &socket, &Deadline::new())).await?;
    assert!(outcome.is_ok());
    assert_next_probe_is_not_blocked(&journal).await
}

#[tokio::test]
async fn create_requested_row_without_container_ends_terminal_and_unblocks_the_next_probe()
-> TestResult<()> {
    Box::pin(timed_out_create_without_container_unblocks(false)).await
}

#[tokio::test]
async fn quarantined_row_without_container_ends_terminal_and_unblocks_the_next_probe()
-> TestResult<()> {
    Box::pin(timed_out_create_without_container_unblocks(true)).await
}

#[tokio::test]
async fn container_appearing_while_absence_settles_is_not_abandoned() -> TestResult<()> {
    let dir = tempfile::tempdir()?;
    let socket = dir.path().join("docker.sock");
    let inspections = start_daemon(&socket, ENGINE_ID, 1)?;
    let (journal, root) = journal_with_create_requested(dir.path(), false).await?;
    let outcome = Box::pin(recover_first(&journal, &root, &socket, &Deadline::new())).await?;
    assert!(outcome.is_err());
    assert!(inspections.load(Ordering::SeqCst) >= 2);
    let row = journal.leased_probe().await?.ok_or(HostError::Journal)?;
    assert_eq!(row.phase, ProbePhase::Quarantined);
    Ok(())
}

#[tokio::test]
async fn absence_on_another_engine_leaves_the_row_untouched() -> TestResult<()> {
    let dir = tempfile::tempdir()?;
    let socket = dir.path().join("docker.sock");
    start_daemon(&socket, "another-engine", usize::MAX)?;
    let (journal, root) = journal_with_create_requested(dir.path(), false).await?;
    let outcome = Box::pin(recover_first(&journal, &root, &socket, &Deadline::new())).await?;
    assert!(outcome.is_err());
    let row = journal.leased_probe().await?.ok_or(HostError::Journal)?;
    assert_eq!(row.phase, ProbePhase::CreateRequested);
    assert_eq!(row.container_id, None);
    Ok(())
}

#[tokio::test]
async fn absence_without_time_to_settle_leaves_the_row_untouched() -> TestResult<()> {
    let dir = tempfile::tempdir()?;
    let socket = dir.path().join("docker.sock");
    start_daemon(&socket, ENGINE_ID, usize::MAX)?;
    let (journal, root) = journal_with_create_requested(dir.path(), false).await?;
    let short = Deadline::until(Instant::now() + Duration::from_secs(5));
    let outcome = Box::pin(recover_first(&journal, &root, &socket, &short)).await?;
    assert!(matches!(outcome, Err(HostError::DockerTimeout)));
    let row = journal.leased_probe().await?.ok_or(HostError::Journal)?;
    assert_eq!(row.phase, ProbePhase::CreateRequested);
    Ok(())
}

#[tokio::test]
async fn arm64_create_requested_row_is_recovered_from_the_journal() -> TestResult<()> {
    let dir = tempfile::tempdir()?;
    let socket = dir.path().join("docker.sock");
    start_daemon_on(&socket, ENGINE_ID, usize::MAX, "aarch64")?;
    let (journal, root) = journal_with_create_requested_on(dir.path(), false, ARM64).await?;
    let outcome = Box::pin(recover_first(&journal, &root, &socket, &Deadline::new())).await?;
    assert_eq!(outcome, Ok(()));
    assert_next_probe_is_not_blocked(&journal).await
}
