// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::HashMap;
use std::sync::Mutex;

use bollard::Docker;
use bollard::errors::Error;
use bollard::query_parameters::{
    ListContainersOptions, ListVolumesOptions, RemoveContainerOptions, RemoveImageOptions,
    RemoveVolumeOptions,
};
use velnor_runner_host::local::ENVIRONMENT_LABEL;

pub(crate) struct Guard {
    environment: String,
    image: String,
    networks: Mutex<Vec<String>>,
    finished: bool,
}

impl Guard {
    pub(crate) fn new(environment: &str, image: &str, network: &str) -> Self {
        Self {
            environment: environment.to_owned(),
            image: image.to_owned(),
            networks: Mutex::new(vec![network.to_owned()]),
            finished: false,
        }
    }

    pub(crate) fn track_network(&self, network: &str) {
        if let Ok(mut networks) = self.networks.lock() {
            networks.push(network.to_owned());
        }
    }

    pub(crate) async fn finish(&mut self, docker: &Docker) -> super::TestResult<()> {
        let networks = self
            .networks
            .get_mut()
            .map_err(|_| "network lock poisoned")?;
        cleanup(docker, &self.environment, &self.image, networks).await?;
        self.finished = true;
        Ok(())
    }
}

impl Drop for Guard {
    fn drop(&mut self) {
        if self.finished {
            return;
        }
        let environment = self.environment.clone();
        let image = self.image.clone();
        let networks = self
            .networks
            .lock()
            .map(|networks| networks.clone())
            .unwrap_or_default();
        let sweep = std::thread::spawn(move || sweep(&environment, &image, &networks));
        let _joined = sweep.join();
    }
}

fn sweep(environment: &str, image: &str, networks: &[String]) {
    let Ok(runtime) = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    else {
        return;
    };
    runtime.block_on(async {
        let Ok(docker) = super::docker() else {
            return;
        };
        let _removed = cleanup(&docker, environment, image, networks).await;
    });
}

fn removed(result: Result<(), Error>) -> super::TestResult<()> {
    match result {
        Ok(())
        | Err(Error::DockerResponseServerError {
            status_code: 404, ..
        }) => Ok(()),
        Err(error) => Err(error.into()),
    }
}

async fn cleanup(
    docker: &Docker,
    environment: &str,
    image: &str,
    networks: &[String],
) -> super::TestResult<()> {
    let filters = HashMap::from([(
        "label".to_owned(),
        vec![format!("{ENVIRONMENT_LABEL}={environment}")],
    )]);
    let containers = docker
        .list_containers(Some(ListContainersOptions {
            all: true,
            filters: Some(filters.clone()),
            ..Default::default()
        }))
        .await?;
    for container in containers.into_iter().filter_map(|container| container.id) {
        removed(
            docker
                .remove_container(
                    &container,
                    Some(RemoveContainerOptions {
                        force: true,
                        v: true,
                        ..Default::default()
                    }),
                )
                .await,
        )?;
    }
    let volumes = docker
        .list_volumes(Some(ListVolumesOptions {
            filters: Some(filters),
        }))
        .await?;
    for volume in volumes.volumes.unwrap_or_default() {
        removed(
            docker
                .remove_volume(&volume.name, None::<RemoveVolumeOptions>)
                .await,
        )?;
    }
    for network in networks {
        removed(docker.remove_network(network).await)?;
    }
    removed(
        docker
            .remove_image(image, None::<RemoveImageOptions>, None)
            .await
            .map(|_| ()),
    )?;
    Ok(())
}
