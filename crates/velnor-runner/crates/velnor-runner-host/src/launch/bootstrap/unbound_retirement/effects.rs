// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use bollard::query_parameters::{RemoveContainerOptions, StopContainerOptions};

use crate::error::HostError;

use super::{Observation, ReadyEffects, inspect_container, inspect_volume, observation, within};

impl ReadyEffects<'_> {
    pub(super) async fn remove_containers(&self) -> Result<Vec<Observation>, HostError> {
        let mut observations = Vec::new();
        for container in &self.inventory.containers {
            let id = container.id.as_deref().ok_or(HostError::Ownership)?;
            let present = self.authorized(inspect_container(self.docker, id)).await?;
            let by_name = self
                .authorized(inspect_container(self.docker, &container.name))
                .await?;
            if by_name
                .as_ref()
                .is_some_and(|named| named.id.as_deref() != Some(id))
            {
                return Err(HostError::Ownership);
            }
            let Some(inspected) = present else {
                if by_name.is_some() {
                    return Err(HostError::Ownership);
                }
                observations.push(observation(
                    &container.role,
                    &container.name,
                    Some(id),
                    "already_absent",
                ));
                continue;
            };
            self.validate_container(&inspected, &container.role, &container.name, id)?;
            if inspected.state.as_ref().and_then(|state| state.running) == Some(true) {
                let stop = self.docker.stop_container(
                    id,
                    Some(StopContainerOptions {
                        t: Some(10),
                        signal: None,
                    }),
                );
                Box::pin(
                    self.authorized(async { within(stop).await?.map_err(|_| HostError::Docker) }),
                )
                .await?;
            }
            let remove = self.docker.remove_container(
                id,
                Some(RemoveContainerOptions {
                    force: false,
                    v: false,
                    link: false,
                }),
            );
            Box::pin(
                self.authorized(async { within(remove).await?.map_err(|_| HostError::Docker) }),
            )
            .await?;
            if self
                .authorized(inspect_container(self.docker, id))
                .await?
                .is_some()
                || self
                    .authorized(inspect_container(self.docker, &container.name))
                    .await?
                    .is_some()
            {
                return Err(HostError::Ownership);
            }
            observations.push(observation(
                &container.role,
                &container.name,
                Some(id),
                "removed_and_absent",
            ));
        }
        if let Some(helper) = self.claim.initializer() {
            observations.push(Box::pin(self.remove_initializer(helper)).await?);
        }
        Ok(observations)
    }

    pub(super) async fn remove_volumes(&self) -> Result<Vec<Observation>, HostError> {
        let mut observations = Vec::new();
        for volume in &self.inventory.volumes {
            let Some(inspected) = self
                .authorized(inspect_volume(self.docker, &volume.name))
                .await?
            else {
                observations.push(observation(
                    &volume.role,
                    &volume.name,
                    None,
                    "already_absent",
                ));
                continue;
            };
            self.validate_volume(&inspected, &volume.role, &volume.name)?;
            let remove = self.docker.remove_volume(
                &volume.name,
                None::<bollard::query_parameters::RemoveVolumeOptions>,
            );
            Box::pin(
                self.authorized(async { within(remove).await?.map_err(|_| HostError::Docker) }),
            )
            .await?;
            if self
                .authorized(inspect_volume(self.docker, &volume.name))
                .await?
                .is_some()
            {
                return Err(HostError::Ownership);
            }
            observations.push(observation(
                &volume.role,
                &volume.name,
                None,
                "removed_and_absent",
            ));
        }
        Ok(observations)
    }
}
