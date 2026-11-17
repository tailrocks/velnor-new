// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::HashMap;
use std::sync::Mutex;

use bollard::query_parameters::{
    ListContainersOptions, ListVolumesOptions, RemoveContainerOptions, RemoveVolumeOptions,
};
use velnor_runner_host::local::ENVIRONMENT_LABEL;

pub(crate) struct Guard {
    environment: String,
    image: String,
    networks: Mutex<Vec<String>>,
}

impl Guard {
    pub(crate) fn new(environment: &str, image: &str, network: &str) -> Self {
        Self {
            environment: environment.to_owned(),
            image: image.to_owned(),
            networks: Mutex::new(vec![network.to_owned()]),
        }
    }

    pub(crate) fn track_network(&self, network: &str) {
        if let Ok(mut networks) = self.networks.lock() {
            networks.push(network.to_owned());
        }
    }
}

impl Drop for Guard {
    fn drop(&mut self) {
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
            .await
            .unwrap_or_default();
        for container in containers.into_iter().filter_map(|container| container.id) {
            let _removed = docker
                .remove_container(
                    &container,
                    Some(RemoveContainerOptions {
                        force: true,
                        v: true,
                        ..Default::default()
                    }),
                )
                .await;
        }
        let volumes = docker
            .list_volumes(Some(ListVolumesOptions {
                filters: Some(filters),
            }))
            .await
            .map(|listed| listed.volumes.unwrap_or_default())
            .unwrap_or_default();
        for volume in volumes {
            let _removed = docker
                .remove_volume(&volume.name, None::<RemoveVolumeOptions>)
                .await;
        }
        for network in networks {
            let _removed = docker.remove_network(network).await;
        }
        let _removed = docker
            .remove_image(image, Some(super::untag_only()), None)
            .await;
    });
}
