//! Bounded measurements of the selected Docker Linux guest.
//!
//! Identity-owned Docker volumes and bounded DinD readiness checks.

use std::collections::HashMap;
use std::time::Duration;

use bollard::Docker;
use bollard::exec::{CreateExecOptions, StartExecOptions, StartExecResults};
use bollard::models::VolumeCreateRequest;
use bollard::query_parameters::RemoveVolumeOptionsBuilder;
use tokio::time::{Instant, sleep, timeout};

use crate::action_archive_seed::ActionArchiveLease;
use crate::error::HostError;
use crate::launch_identity::LaunchIdentity;
use crate::stage::{ContainerRecord, DindProbe};

mod containers;
pub(crate) mod guest;
pub(crate) use containers::refuse_existing;

const DOCKER_CALL_TIMEOUT: Duration = Duration::from_secs(5);
const VOLUME_CREATE_TIMEOUT: Duration = Duration::from_secs(10);
const EXEC_DEADLINE: Duration = Duration::from_secs(4);
const EXEC_POLL: Duration = Duration::from_millis(100);
const STORAGE_PROBE: &str = "info=$(timeout 2s docker info --format '{{.Driver}}|{{.DockerRootDir}}' 2>/dev/null) || exit 75; [ \"$info\" = 'vfs|/var/lib/docker' ] || exit 76";

#[derive(Debug, Clone, Copy)]
pub(super) struct PrivateVolume {
    pub(super) suffix: &'static str,
    pub(super) kind: &'static str,
}

const PRIVATE_VOLUMES: [PrivateVolume; 3] = [
    PrivateVolume {
        suffix: "",
        kind: "runner-socket",
    },
    PrivateVolume {
        suffix: "-work",
        kind: "runner-workspace",
    },
    PrivateVolume {
        suffix: "-docker",
        kind: "dind-data",
    },
];

pub(crate) async fn create_owned_volumes(
    docker: &Docker,
    identity: &LaunchIdentity,
) -> Result<(), HostError> {
    verify_engine(docker, identity).await?;
    for volume in PRIVATE_VOLUMES {
        create_owned_volume(docker, identity, volume).await?;
    }
    Ok(())
}

pub(crate) async fn remove_owned_volumes(
    docker: &Docker,
    identity: &LaunchIdentity,
) -> Result<(), HostError> {
    verify_engine(docker, identity).await?;
    for volume in PRIVATE_VOLUMES.into_iter().rev() {
        remove_owned_volume(docker, identity, volume).await?;
    }
    Ok(())
}

async fn create_owned_volume(
    docker: &Docker,
    identity: &LaunchIdentity,
    volume: PrivateVolume,
) -> Result<(), HostError> {
    let name = volume_name(identity, volume);
    let request = VolumeCreateRequest {
        name: Some(name),
        labels: Some(volume_labels(identity, volume)),
        ..Default::default()
    };
    timeout(VOLUME_CREATE_TIMEOUT, docker.create_volume(request))
        .await
        .map_err(|_| HostError::VolumeCreateUncertain)?
        .map_err(|_| HostError::VolumeCreateUncertain)?;
    verify_volume(docker, identity, volume).await
}

async fn remove_owned_volume(
    docker: &Docker,
    identity: &LaunchIdentity,
    volume: PrivateVolume,
) -> Result<(), HostError> {
    match inspect_volume(docker, identity, volume).await? {
        None => return Ok(()),
        Some(labels) if labels == volume_labels(identity, volume) => {}
        Some(_) => return Err(HostError::Ownership),
    }
    let name = volume_name(identity, volume);
    let options = RemoveVolumeOptionsBuilder::new().force(false).build();
    match timeout(
        DOCKER_CALL_TIMEOUT,
        docker.remove_volume(&name, Some(options)),
    )
    .await
    {
        Ok(Err(error)) if confirmed_not_found(&error) => return Ok(()),
        Err(_) | Ok(Err(_)) => {
            return match inspect_volume(docker, identity, volume).await? {
                None => Ok(()),
                Some(_) => Err(HostError::Cleanup),
            };
        }
        Ok(Ok(())) => {}
    }
    if inspect_volume(docker, identity, volume).await?.is_some() {
        return Err(HostError::Cleanup);
    }
    Ok(())
}

async fn verify_volume(
    docker: &Docker,
    identity: &LaunchIdentity,
    volume: PrivateVolume,
) -> Result<(), HostError> {
    let Some(labels) = inspect_volume(docker, identity, volume).await? else {
        return Err(HostError::Ownership);
    };
    if labels == volume_labels(identity, volume) {
        Ok(())
    } else {
        Err(HostError::Ownership)
    }
}

async fn inspect_volume(
    docker: &Docker,
    identity: &LaunchIdentity,
    volume: PrivateVolume,
) -> Result<Option<HashMap<String, String>>, HostError> {
    let name = volume_name(identity, volume);
    let result = timeout(DOCKER_CALL_TIMEOUT, docker.inspect_volume(&name))
        .await
        .map_err(|_| HostError::DockerTimeout)?;
    match result {
        Ok(found) => Ok(Some(found.labels)),
        Err(error) if confirmed_not_found(&error) => Ok(None),
        Err(_) => Err(HostError::Docker),
    }
}

pub(crate) async fn verify_engine(
    docker: &Docker,
    identity: &LaunchIdentity,
) -> Result<(), HostError> {
    let info = timeout(DOCKER_CALL_TIMEOUT, docker.info())
        .await
        .map_err(|_| HostError::DockerTimeout)?
        .map_err(|_| HostError::Docker)?;
    if info.id.as_deref() == Some(identity.engine_id()) {
        Ok(())
    } else {
        Err(HostError::Ownership)
    }
}

pub(crate) async fn probe_dind(docker: &Docker, dind_id: &str) -> Result<DindProbe, HostError> {
    let created = create_probe(docker, dind_id).await?;
    start_probe(docker, &created.id).await?;
    await_exec_result(docker, &created.id).await
}

pub(crate) async fn list_launch(
    docker: &Docker,
    identity: &LaunchIdentity,
) -> Result<Vec<ContainerRecord>, HostError> {
    verify_engine(docker, identity).await?;
    containers::list_launch(docker, identity).await
}

pub(crate) async fn verify_container(
    docker: &Docker,
    identity: &LaunchIdentity,
    role: &str,
    id: &str,
    dind_id: Option<&str>,
    archive_lease: Option<&ActionArchiveLease>,
    require_running: bool,
) -> Result<ContainerRecord, HostError> {
    verify_engine(docker, identity).await?;
    ensure_owned_volumes(docker, identity).await?;
    containers::verify_container(
        docker,
        identity,
        role,
        id,
        dind_id,
        archive_lease,
        require_running,
    )
    .await
}

async fn ensure_owned_volumes(docker: &Docker, identity: &LaunchIdentity) -> Result<(), HostError> {
    for volume in PRIVATE_VOLUMES {
        verify_volume(docker, identity, volume).await?;
    }
    Ok(())
}

async fn create_probe(
    docker: &Docker,
    dind_id: &str,
) -> Result<bollard::exec::CreateExecResults, HostError> {
    let options = CreateExecOptions::<String> {
        attach_stdin: Some(false),
        attach_stdout: Some(false),
        attach_stderr: Some(false),
        tty: Some(false),
        cmd: Some(vec![
            "/bin/sh".to_owned(),
            "-c".to_owned(),
            STORAGE_PROBE.to_owned(),
        ]),
        ..Default::default()
    };
    timeout(DOCKER_CALL_TIMEOUT, docker.create_exec(dind_id, options))
        .await
        .map_err(|_| HostError::DockerTimeout)?
        .map_err(|_| HostError::Docker)
}

async fn start_probe(docker: &Docker, exec_id: &str) -> Result<(), HostError> {
    let started = timeout(
        DOCKER_CALL_TIMEOUT,
        docker.start_exec(
            exec_id,
            Some(StartExecOptions {
                detach: true,
                tty: false,
                output_capacity: None,
            }),
        ),
    )
    .await
    .map_err(|_| HostError::DockerTimeout)?
    .map_err(|_| HostError::Docker)?;
    if matches!(started, StartExecResults::Detached) {
        Ok(())
    } else {
        Err(HostError::DindReadiness)
    }
}

async fn await_exec_result(docker: &Docker, exec_id: &str) -> Result<DindProbe, HostError> {
    let deadline = Instant::now() + EXEC_DEADLINE;
    loop {
        if Instant::now() >= deadline {
            return Err(HostError::DockerTimeout);
        }
        let state = timeout(DOCKER_CALL_TIMEOUT, docker.inspect_exec(exec_id))
            .await
            .map_err(|_| HostError::DockerTimeout)?
            .map_err(|_| HostError::Docker)?;
        match probe_result(state.running, state.exit_code) {
            Ok(Some(probe)) => return Ok(probe),
            Ok(None) => sleep(EXEC_POLL).await,
            Err(error) => return Err(error),
        }
    }
}

pub(super) fn probe_result(
    running: Option<bool>,
    exit_code: Option<i64>,
) -> Result<Option<DindProbe>, HostError> {
    match (running, exit_code) {
        (Some(true), _) => Ok(None),
        (Some(false), Some(0)) => Ok(Some(DindProbe::Ready)),
        (Some(false), Some(75)) => Ok(Some(DindProbe::Starting)),
        (Some(false), Some(76)) => Err(HostError::DindStorage),
        (Some(false), Some(_)) | (None, _) | (_, None) => Err(HostError::DindReadiness),
    }
}

pub(super) fn volume_name(identity: &LaunchIdentity, volume: PrivateVolume) -> String {
    format!(
        "{}{suffix}",
        identity.private_volume(),
        suffix = volume.suffix
    )
}

pub(super) fn volume_labels(
    identity: &LaunchIdentity,
    volume: PrivateVolume,
) -> HashMap<String, String> {
    HashMap::from([
        ("velnor.product".to_owned(), "velnor".to_owned()),
        (
            "velnor.instance".to_owned(),
            identity.instance_id().to_owned(),
        ),
        ("velnor.launch".to_owned(), identity.launch_id().to_owned()),
        ("velnor.engine".to_owned(), identity.engine_id().to_owned()),
        (
            "velnor.owner".to_owned(),
            identity.private_volume().to_owned(),
        ),
        ("velnor.kind".to_owned(), volume.kind.to_owned()),
    ])
}

/// Accept only an explicit Docker 404 as proof that a resource is absent.
pub(crate) fn confirmed_not_found(error: &bollard::errors::Error) -> bool {
    matches!(
        error,
        bollard::errors::Error::DockerResponseServerError {
            status_code: 404,
            ..
        }
    )
}
