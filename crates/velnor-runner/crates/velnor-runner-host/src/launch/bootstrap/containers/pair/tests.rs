use super::{
    JIT_MAX_BYTES, RunnerOutput, StartedBootstrapPair, fixed_mounts, limits_for, tmpfs_options,
    valid_jit_payload,
};
use crate::error::HostError;
use crate::launch::bootstrap::BootstrapContainerRole;
use crate::worker::{WorkerIdentityNames, WorkerVolumeRole};
use std::io::ErrorKind;
use std::os::unix::net::UnixListener;
use std::time::{Duration, Instant};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use velnor_runner_github::EncodedJit;

async fn delivery_fixture(
    nonce: char,
    buffer: usize,
) -> Result<
    (
        crate::journal::request_lifecycle::tests::helpers::ListenerDeliveryFixture,
        StartedBootstrapPair,
        tokio::io::DuplexStream,
    ),
    crate::error::HostError,
> {
    let fixture =
        crate::journal::request_lifecycle::tests::helpers::listener_delivery_fixture(nonce).await?;
    let claim = &fixture.claim;
    let (writer, reader) = tokio::io::duplex(buffer);
    let pair = StartedBootstrapPair {
        receipt: super::super::BootstrapStartReceipt {
            reservation_id: claim.reservation_id(),
            operation_id: claim.operation_id(),
            engine_id: claim.engine_id().to_owned(),
            docker_root_digest: claim.docker_root_digest().to_owned(),
            runner_container_id: claim.runner_container_id().to_owned(),
            dind_container_id: "d".repeat(64),
            runner_stdin_open: true,
            jit_withheld: true,
        },
        mount_handoff: crate::launch::bootstrap::VerifiedBootstrapMountHandoff {
            connection_id: "test-connection".to_owned(),
            reservation_id: claim.reservation_id(),
            operation_id: claim.operation_id(),
            engine_id: claim.engine_id().to_owned(),
            docker_root_digest: claim.docker_root_digest().to_owned(),
            metadata_volume_name: "test-metadata".to_owned(),
            metadata_volume_id: "test-metadata".to_owned(),
            layout_digest: "a".repeat(64),
            runner_container_id: claim.runner_container_id().to_owned(),
            dind_container_id: "d".repeat(64),
            mount_evidence_digest: "b".repeat(64),
        },
        stdin: Box::pin(writer),
        output: Some(Box::pin(futures_util::stream::empty::<
            Result<bollard::container::LogOutput, bollard::errors::Error>,
        >()) as RunnerOutput),
        stdin_open: true,
        jit_withheld: true,
        delivery_attempted: false,
        delivery_succeeded: false,
    };
    Ok((fixture, pair, reader))
}

#[test]
fn jit_payload_respects_linux_environment_entry_bound() {
    assert!(valid_jit_payload(&vec![b'a'; JIT_MAX_BYTES]));
    assert!(!valid_jit_payload(&vec![b'a'; JIT_MAX_BYTES + 1]));
    assert!(!valid_jit_payload(b""));
    assert!(!valid_jit_payload(b"a\0b"));
    assert!(!valid_jit_payload("é".as_bytes()));
}

#[test]
fn runner_mounts_use_only_owned_job_volumes_and_metadata_paths() {
    let names = WorkerIdentityNames::from_nonce("0123456789abcdef0123456789abcdef")
        .expect("valid runner name");
    let mounts = fixed_mounts(&names, BootstrapContainerRole::Runner).expect("mounts");
    let targets = mounts
        .iter()
        .map(|mount| mount.target.as_deref().expect("target"))
        .collect::<Vec<_>>();
    assert_eq!(targets.len(), 7);
    assert!(targets.contains(&"/home/runner"));
    assert!(targets.contains(&"/home/runner/_work"));
    assert!(targets.contains(&"/run"));
    assert!(targets.contains(&"/.velnor-runtime-identity"));
    assert!(targets.contains(&"/opt/velnor/job-facts"));
    assert!(mounts.iter().all(|mount| {
        mount
            .volume_options
            .as_ref()
            .and_then(|options| options.no_copy)
            == Some(true)
    }));
    let metadata = names.volume_name(WorkerVolumeRole::RunnerMetadata);
    let metadata_mounts = mounts
        .iter()
        .filter(|mount| mount.source.as_deref() == Some(metadata.as_str()))
        .collect::<Vec<_>>();
    assert_eq!(metadata_mounts.len(), 3);
    for (subpath, target) in [
        ("runtime-identity", "/.velnor-runtime-identity"),
        ("job-facts", "/opt/velnor/job-facts"),
        ("source-proof/control", "/opt/velnor/source-proof/control"),
    ] {
        let mount = metadata_mounts
            .iter()
            .find(|mount| mount.target.as_deref() == Some(target))
            .expect("metadata subpath mount");
        let options = mount.volume_options.as_ref().expect("volume options");
        assert_eq!(options.subpath.as_deref(), Some(subpath));
        assert_eq!(mount.read_only, Some(true));
    }
    let home = names.volume_name(WorkerVolumeRole::Home);
    let inbox = mounts
        .iter()
        .find(|mount| mount.target.as_deref() == Some("/opt/velnor/source-proof/inbox"))
        .expect("RunnerHome inbox subpath");
    assert_eq!(inbox.source.as_deref(), Some(home.as_str()));
    assert_eq!(
        inbox
            .volume_options
            .as_ref()
            .and_then(|options| options.subpath.as_deref()),
        Some("source-proof/inbox")
    );
    assert_eq!(inbox.read_only, Some(false));
}

#[test]
fn tmpfs_capacity_uses_finite_reserved_values() {
    assert_eq!(
        tmpfs_options(4096, 128).expect("bounded tmpfs"),
        "rw,nosuid,nodev,size=4096,nr_inodes=128"
    );
    assert!(tmpfs_options(0, 128).is_err());
    assert!(tmpfs_options(4096, 0).is_err());
}

#[test]
fn only_dind_gets_privilege_and_its_pid_limit_is_finite() {
    assert_eq!(limits_for(BootstrapContainerRole::Runner), (1024, false));
    assert_eq!(limits_for(BootstrapContainerRole::Dind), (2048, true));
}

#[test]
fn dind_mounts_private_storage_and_never_mounts_the_host_socket() {
    let names = WorkerIdentityNames::from_nonce("0123456789abcdef0123456789abcdef")
        .expect("valid runner name");
    let mounts = fixed_mounts(&names, BootstrapContainerRole::Dind).expect("mounts");
    let targets = mounts
        .iter()
        .map(|mount| mount.target.as_deref().expect("target"))
        .collect::<Vec<_>>();
    assert_eq!(targets.len(), 6);
    assert!(targets.contains(&"/var/lib/docker"));
    assert!(targets.contains(&"/opt/velnor/job-facts"));
    assert!(targets.contains(&"/.velnor-runtime-identity"));
    assert!(!targets.contains(&"/var/run/docker.sock"));
    assert!(mounts.iter().all(|mount| {
        mount
            .volume_options
            .as_ref()
            .and_then(|options| options.no_copy)
            == Some(true)
    }));
    let metadata = names.volume_name(WorkerVolumeRole::RunnerMetadata);
    let metadata_mounts = mounts
        .iter()
        .filter(|mount| mount.source.as_deref() == Some(metadata.as_str()))
        .collect::<Vec<_>>();
    assert_eq!(metadata_mounts.len(), 2);
    for (subpath, target) in [
        ("job-facts", "/opt/velnor/job-facts"),
        ("runtime-identity", "/.velnor-runtime-identity"),
    ] {
        let mount = metadata_mounts
            .iter()
            .find(|mount| mount.target.as_deref() == Some(target))
            .expect("The mount subpath must be present.");
        assert_eq!(mount.source.as_deref(), Some(metadata.as_str()));
        assert_eq!(mount.read_only, Some(true));
        let options = mount.volume_options.as_ref().expect("volume options");
        assert_eq!(options.no_copy, Some(true));
        assert_eq!(options.subpath.as_deref(), Some(subpath));
    }
    assert!(mounts.iter().all(|mount| {
        mount.source.as_deref() == Some(metadata.as_str()) || mount.read_only == Some(false)
    }));
    assert_eq!(
        mounts
            .iter()
            .find(|mount| mount.target.as_deref() == Some("/var/lib/docker"))
            .and_then(|mount| mount.source.as_deref()),
        Some(names.volume_name(WorkerVolumeRole::DindData).as_str())
    );
}

#[tokio::test]
async fn claimed_jit_write_closes_stdin_and_returns_exact_delivery_proof() {
    let (fixture, mut pair, mut reader) = delivery_fixture('c', 64).await.expect("fixture");
    let bytes = b"encoded-jit";
    let receipt = pair
        .deliver_jit(
            &fixture.claim,
            EncodedJit::new(String::from_utf8(bytes.to_vec()).expect("ascii")),
            Instant::now() + Duration::from_secs(1),
        )
        .await
        .expect("delivery");
    let mut observed = Vec::new();
    reader.read_to_end(&mut observed).await.expect("read stdin");
    assert_eq!(observed, bytes);
    assert!(receipt.delivered());
    assert!(receipt.jit_bytes_written());
    assert!(receipt.stdin_closed());
    assert!(!pair.runner_stdin_open());
    assert!(!pair.jit_withheld());
    assert!(pair.take_output().is_some());
}

#[tokio::test]
async fn timed_out_jit_write_poisons_pair_and_cannot_be_retried() {
    let (fixture, mut pair, reader) = delivery_fixture('d', 1).await.expect("fixture");
    let result = pair
        .deliver_jit(
            &fixture.claim,
            EncodedJit::new("encoded-jit-that-exceeds-buffer".to_owned()),
            Instant::now() + Duration::from_millis(10),
        )
        .await;
    assert!(matches!(
        result,
        Err(crate::error::HostError::DockerTimeout)
    ));
    assert!(pair.runner_stdin_open());
    assert!(pair.jit_withheld());
    assert!(pair.take_output().is_none());
    assert!(
        pair.deliver_jit(
            &fixture.claim,
            EncodedJit::new("retry".to_owned()),
            Instant::now() + Duration::from_secs(1),
        )
        .await
        .is_err()
    );
    drop(reader);
}

#[tokio::test]
async fn mismatched_container_identity_rejects_delivery_before_write() {
    let (fixture, mut pair, mut reader) = delivery_fixture('e', 64).await.expect("fixture");
    pair.receipt.runner_container_id = "e".repeat(64);
    assert!(matches!(
        pair.deliver_jit(
            &fixture.claim,
            EncodedJit::new("encoded-jit".to_owned()),
            Instant::now() + Duration::from_secs(1),
        )
        .await,
        Err(crate::error::HostError::Identity)
    ));
    drop(pair);
    let mut observed = Vec::new();
    reader.read_to_end(&mut observed).await.expect("read stdin");
    assert_eq!(observed, Vec::<u8>::new());
}

fn expired_start_docker(
    scratch: &crate::journal::request_lifecycle::tests::helpers::Scratch,
) -> Result<(bollard::Docker, UnixListener), HostError> {
    let socket_path = scratch.file().with_file_name("s.sock");
    let listener = UnixListener::bind(&socket_path).map_err(|_| HostError::Docker)?;
    listener
        .set_nonblocking(true)
        .map_err(|_| HostError::Docker)?;
    let socket = socket_path.to_str().ok_or(HostError::Path)?;
    let docker = bollard::Docker::connect_with_unix(socket, 120, bollard::API_DEFAULT_VERSION)
        .map_err(|_| HostError::Docker)?;
    Ok((docker, listener))
}

async fn expired_start_state() -> Result<
    (
        crate::journal::request_lifecycle::tests::helpers::Scratch,
        crate::journal::ConnectionJournal,
        crate::journal::request_lifecycle::tests::helpers::BootstrapStartClaimFixture,
    ),
    HostError,
> {
    use crate::journal::request_lifecycle::tests::helpers::{
        Scratch, binding, created_scope, prepare_bootstrap_start_claim_for_nonce,
    };
    use crate::journal::{CapacityLimits, Journal};

    let scratch = Scratch::new()?;
    let journal = Journal::open(&scratch.file()).await?;
    journal
        .configure_capacity(CapacityLimits {
            jobs: 1,
            cpu_millicores: 2_000,
            memory_bytes: 25_769_803_776,
        })
        .await?;
    let repository = binding("pair-start-retain", 9_901)?;
    let scope = created_scope(&journal, &repository, 9_902).await?;
    let claim = prepare_bootstrap_start_claim_for_nonce(&scope, 'f').await?;
    Ok((scratch, scope, claim))
}

#[tokio::test]
async fn failed_start_keeps_claim_and_runner_attach_until_recovery() -> Result<(), HostError> {
    use crate::journal::EngineContext;
    use crate::journal::request_lifecycle::tests::helpers::bootstrap_start_state;
    use crate::launch::{SelectedEngineContext, verified_images::test_worker_images_for};

    let (scratch, scope, fixture) = expired_start_state().await?;
    let reservation_id = fixture.reservation_id;
    let crate::journal::request_lifecycle::tests::helpers::BootstrapStartClaimFixture {
        claim,
        metadata_handoff,
        engine_id,
        docker_root_digest,
        ..
    } = fixture;
    let images = test_worker_images_for(&engine_id, &docker_root_digest, "aarch64", "linux/amd64");
    let selected = SelectedEngineContext::bind(
        &EngineContext {
            engine_id: engine_id.clone(),
            endpoint: "unix:///var/run/docker.sock".to_owned(),
            platform: "linux/amd64".to_owned(),
        },
        &images,
    )?;
    let (docker, listener) = expired_start_docker(&scratch)?;
    let (writer, mut reader) = tokio::io::duplex(64);
    let attached = bollard::container::AttachContainerResults {
        output: Box::pin(futures_util::stream::empty()),
        input: Box::pin(writer),
    };
    let names = WorkerIdentityNames::from_nonce(&"f".repeat(32))?;
    let runner_mounts = fixed_mounts(&names, BootstrapContainerRole::Runner)?;
    let dind_mounts = fixed_mounts(&names, BootstrapContainerRole::Dind)?;
    let result = super::production::start_attached_pair(
        &docker,
        &scope,
        &selected,
        &images,
        claim,
        attached,
        &metadata_handoff,
        &runner_mounts,
        &dind_mounts,
        Instant::now() - Duration::from_millis(1),
    )
    .await;
    assert!(result.is_err(), "expired start deadline must fail closed");
    let failure = result.err().ok_or(HostError::Docker)?;
    assert!(
        matches!(listener.accept(), Err(error) if error.kind() == ErrorKind::WouldBlock),
        "expired start deadline must not send a Docker request"
    );
    let (error, pending) = failure.into_parts();
    assert!(matches!(error, HostError::Docker));
    let mut pending = pending.expect("attached pair must remain owned");
    assert_eq!(
        bootstrap_start_state(&scope, reservation_id).await?,
        "uncertain"
    );
    let crate::launch::bootstrap::containers::PendingBootstrapPair {
        _attached: attached,
        ..
    } = &mut pending;
    attached
        .input
        .write_all(b"still-owned")
        .await
        .map_err(|_| HostError::Docker)?;
    let mut bytes = [0_u8; 11];
    reader
        .read_exact(&mut bytes)
        .await
        .map_err(|_| HostError::Docker)?;
    assert_eq!(&bytes, b"still-owned");
    Ok(())
}
