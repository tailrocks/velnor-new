//! Give the job work volume to the runner user before the listener starts.
//!
//! A new named volume is root-owned mode `0755`. The runner uid is `1000`.
//! `CreateDirectory("/home/runner/_work/_tool")` then fails.

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

/// `chown runner:runner` on `{volume}` mounted at the runner work path.
///
/// The container is root, unprivileged, and has no network. It is removed
/// before this function returns.
///
/// # Errors
///
/// Returns [`HostError::ForbiddenMount`] when `volume` is not a private work
/// volume. Returns [`HostError::Docker`] when create, start, or `chown` fails.
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

/// Create body for the one-shot owner. No network and no privilege.
///
/// # Errors
///
/// Returns [`HostError::ForbiddenMount`] when `volume` is not a private
/// `*-work` name.
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
    if docker.remove_container(id, Some(options)).await.is_err() {
        // The chown result still stands when removal fails.
    }
}

#[cfg(test)]
mod tests {
    use velnor_runner_core::runner_work_path;

    use super::chown_body;
    use crate::docker_spec::RUNNER_IMAGE;

    #[test]
    fn chown_runs_as_root_on_the_work_volume_only() {
        let work = runner_work_path();
        let body = chown_body("worker_a-work").expect("work volume");
        assert_eq!(body.image.as_deref(), Some(RUNNER_IMAGE));
        assert_eq!(body.user.as_deref(), Some("0:0"));
        assert_eq!(
            body.entrypoint.as_deref(),
            Some(["chown".to_owned()].as_slice())
        );
        assert_eq!(
            body.cmd.as_deref(),
            Some(["runner:runner".to_owned(), work.clone()].as_slice())
        );
        assert_eq!(body.network_disabled, Some(true));
        let host = body.host_config.expect("host");
        assert_eq!(host.privileged, Some(false));
        assert_eq!(host.network_mode.as_deref(), Some("none"));
        let mounts = host.mounts.expect("mount");
        assert_eq!(mounts.len(), 1);
        assert_eq!(mounts[0].source.as_deref(), Some("worker_a-work"));
        assert_eq!(mounts[0].target.as_deref(), Some(work.as_str()));
        assert_eq!(
            body.labels
                .expect("labels")
                .get("velnor.role")
                .map(String::as_str),
            Some("work-owner")
        );
    }

    #[test]
    fn chown_rejects_the_socket_volume_and_host_paths() {
        assert!(chown_body("worker_a").is_err());
        assert!(chown_body("-work").is_err());
        assert!(chown_body("../secret-work").is_err());
        assert!(chown_body("/var/lib/docker-work").is_err());
    }
}
