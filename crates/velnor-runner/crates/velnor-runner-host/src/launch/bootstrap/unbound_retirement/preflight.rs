// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use bollard::{errors::Error as DockerError, models::NetworkInspect};

use crate::{error::HostError, journal::UnboundResourceKind};

use super::{
    Effects, ReadyEffects, UnboundCreationObservation, inspect_container, inspect_volume, within,
};

pub(super) enum Publication<'a> {
    Ready(ReadyEffects<'a>),
    Deferred,
}

impl<'a> Effects<'a> {
    pub(super) async fn preflight(self) -> Result<Publication<'a>, HostError> {
        self.authorized(async { Ok(()) }).await?;
        let inventory = self
            .scope
            .refresh_unbound_retirement_inventory(self.claim)
            .await?;
        for container in inventory.containers.iter().filter(|item| item.id.is_none()) {
            let Some(inspected) = self
                .authorized(inspect_container(self.docker, &container.name))
                .await?
            else {
                return Ok(Publication::Deferred);
            };
            let proof = UnboundCreationObservation::container(
                &self,
                &container.role,
                &container.name,
                &inspected,
            )?;
            self.scope
                .record_unbound_creation_target(self.claim, &proof)
                .await?;
        }
        for volume in inventory.volumes.iter().filter(|item| item.id.is_none()) {
            let Some(inspected) = self
                .authorized(inspect_volume(self.docker, &volume.name))
                .await?
            else {
                return Ok(Publication::Deferred);
            };
            let proof =
                UnboundCreationObservation::volume(&self, &volume.role, &volume.name, &inspected)?;
            self.scope
                .record_unbound_creation_target(self.claim, &proof)
                .await?;
        }
        if !self.preflight_network().await? {
            return Ok(Publication::Deferred);
        }
        let inventory = self
            .scope
            .refresh_unbound_retirement_inventory(self.claim)
            .await?;
        Ok(Publication::Ready(ReadyEffects {
            effects: self,
            inventory,
        }))
    }

    async fn preflight_network(&self) -> Result<bool, HostError> {
        let Some(network) = self
            .scope
            .storage_network_recovery_identity(self.claim.reservation_id())
            .await?
        else {
            return Ok(true);
        };
        if network.operation_id() != self.claim.operation_id()
            || network.engine_id() != self.claim.engine_id()
            || network.docker_root_digest() != self.claim.docker_root_digest()
        {
            return Err(HostError::Ownership);
        }
        if network.network_id().is_some()
            || self
                .scope
                .unbound_creation_target(self.claim, UnboundResourceKind::Network, "private")
                .await?
                .is_some()
        {
            return Ok(true);
        }
        let Some(inspected) = self
            .authorized(inspect_network(self.docker, network.network_name()))
            .await?
        else {
            return Ok(false);
        };
        let proof = UnboundCreationObservation::network(self, &network, &inspected)?;
        self.scope
            .record_unbound_creation_target(self.claim, &proof)
            .await?;
        Ok(true)
    }
}

async fn inspect_network(
    docker: &bollard::Docker,
    name: &str,
) -> Result<Option<NetworkInspect>, HostError> {
    match within(docker.inspect_network(name, None)).await? {
        Ok(network) => Ok(Some(network)),
        Err(DockerError::DockerResponseServerError {
            status_code: 404, ..
        }) => Ok(None),
        Err(_) => Err(HostError::Docker),
    }
}
