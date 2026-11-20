// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::HashMap;
use std::sync::Mutex;

use bollard::Docker;
use bollard::errors::Error;
use bollard::query_parameters::{
    ListContainersOptions, ListVolumesOptions, RemoveContainerOptions, RemoveVolumeOptions,
};
use velnor_runner_host::local::ENVIRONMENT_LABEL;

pub(crate) struct Guard {
    docker: Docker,
    environment: String,
    image: String,
    networks: Mutex<Vec<String>>,
    finished: bool,
}

impl Guard {
    pub(crate) fn new(docker: Docker, environment: &str, image: &str, network: &str) -> Self {
        Self {
            docker,
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

    pub(crate) fn image_tag(&self) -> &str {
        &self.image
    }

    pub(crate) async fn finish(&mut self) -> super::TestResult<()> {
        let networks = self
            .networks
            .get_mut()
            .map_err(|_| "network lock poisoned")?;
        cleanup(&self.docker, &self.environment, &self.image, networks).await?;
        self.finished = true;
        Ok(())
    }
}

impl Drop for Guard {
    fn drop(&mut self) {
        if self.finished {
            return;
        }
        let docker = self.docker.clone();
        let environment = self.environment.clone();
        let image = self.image.clone();
        let networks = self
            .networks
            .lock()
            .map(|networks| networks.clone())
            .unwrap_or_default();
        let sweep = std::thread::spawn(move || sweep(&docker, &environment, &image, &networks));
        let _joined = sweep.join();
    }
}

fn sweep(docker: &Docker, environment: &str, image: &str, networks: &[String]) {
    let Ok(runtime) = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    else {
        return;
    };
    runtime.block_on(async {
        let _removed = cleanup(docker, environment, image, networks).await;
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

fn attempted<T>(
    result: super::TestResult<T>,
    first: &mut Option<Box<dyn std::error::Error>>,
) -> Option<T> {
    match result {
        Ok(value) => Some(value),
        Err(error) => {
            if first.is_none() {
                *first = Some(error);
            }
            None
        }
    }
}

async fn cleanup(
    docker: &Docker,
    environment: &str,
    image: &str,
    networks: &[String],
) -> super::TestResult<()> {
    let mut first = None;
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
        .await;
    if let Some(containers) = attempted(containers.map_err(Into::into), &mut first) {
        for container in containers.into_iter().filter_map(|container| container.id) {
            attempted(
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
                ),
                &mut first,
            );
        }
    }
    let volumes = docker
        .list_volumes(Some(ListVolumesOptions {
            filters: Some(filters),
        }))
        .await;
    if let Some(volumes) = attempted(volumes.map_err(Into::into), &mut first) {
        for volume in volumes.volumes.unwrap_or_default() {
            attempted(
                removed(
                    docker
                        .remove_volume(&volume.name, None::<RemoveVolumeOptions>)
                        .await,
                ),
                &mut first,
            );
        }
    }
    for network in networks {
        attempted(removed(docker.remove_network(network).await), &mut first);
    }
    attempted(
        removed(
            docker
                .remove_image(image, Some(super::untag_only()), None)
                .await
                .map(|_| ()),
        ),
        &mut first,
    );
    first.map_or(Ok(()), Err)
}
