use std::collections::HashMap;
use std::time::Duration;

use bollard::Docker;
use bollard::models::{ContainerCreateBody, HostConfig, Mount as DockerMount, MountType};
use bollard::query_parameters::{CreateContainerOptions, RemoveContainerOptionsBuilder};
use velnor_runner_core::runner_work_path;

use crate::docker_spec::{RUNNER_IMAGE, RUNNER_PLATFORM, accepts_volume_name};
use crate::error::HostError;

const ATTEMPTS: u32 = 40;
const PAUSE: Duration = Duration::from_millis(500);

pub(crate) async fn own_work_volume(docker: &Docker, volume: &str) -> Result<(), HostError> {
    let config = chown_body(volume)?;
    let options = CreateContainerOptions {
        name: None,
        platform: RUNNER_PLATFORM.to_owned(),
    };
    let created = docker
        .create_container(Some(options), config)
        .await
        .map_err(|_| HostError::Docker)?;
    if created.id.is_empty() {
        return Err(HostError::Docker);
    }
    let id = created.id;
    if docker
        .start_container(
            &id,
            None::<bollard::query_parameters::StartContainerOptions>,
        )
        .await
        .is_err()
    {
        remove_force(docker, &id).await;
        return Err(HostError::Docker);
    }
    let code = wait_exit(docker, &id).await;
    remove_force(docker, &id).await;
    match code {
        Ok(0) => Ok(()),
        _ => Err(HostError::Docker),
    }
}

pub(crate) fn chown_body(volume: &str) -> Result<ContainerCreateBody, HostError> {
    let name = work_volume(volume)?;
    let work = runner_work_path();
    let mut labels = HashMap::new();
    labels.insert("velnor.role".to_owned(), "work-owner".to_owned());
    Ok(ContainerCreateBody {
        image: Some(RUNNER_IMAGE.to_owned()),
        user: Some("0:0".to_owned()),
        entrypoint: Some(vec!["chown".to_owned()]),
        cmd: Some(vec!["runner:runner".to_owned(), work.clone()]),
        labels: Some(labels),
        network_disabled: Some(true),
        host_config: Some(HostConfig {
            privileged: Some(false),
            network_mode: Some("none".to_owned()),
            mounts: Some(vec![DockerMount {
                target: Some(work),
                source: Some(name.to_owned()),
                typ: Some(MountType::VOLUME),
                ..Default::default()
            }]),
            ..Default::default()
        }),
        ..Default::default()
    })
}

fn work_volume(volume: &str) -> Result<&str, HostError> {
    let parent = volume
        .strip_suffix("-work")
        .ok_or(HostError::ForbiddenMount)?;
    if parent.is_empty() || !accepts_volume_name(parent) {
        return Err(HostError::ForbiddenMount);
    }
    Ok(volume)
}

async fn wait_exit(docker: &Docker, id: &str) -> Result<i64, HostError> {
    for _ in 0..ATTEMPTS {
        let body = docker
            .inspect_container(id, None)
            .await
            .map_err(|_| HostError::Docker)?;
        let running = body.state.as_ref().and_then(|state| state.running);
        if running == Some(false) {
            return body
                .state
                .and_then(|state| state.exit_code)
                .ok_or(HostError::Docker);
        }
        tokio::time::sleep(PAUSE).await;
    }
    Err(HostError::Docker)
}

async fn remove_force(docker: &Docker, id: &str) {
    let options = RemoveContainerOptionsBuilder::new().force(true).build();
    if docker.remove_container(id, Some(options)).await.is_err() {}
}
