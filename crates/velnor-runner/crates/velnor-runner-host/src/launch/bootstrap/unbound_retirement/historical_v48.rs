// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::HashMap;
use std::future::Future;
use std::time::Duration;

use bollard::{
    Docker,
    errors::Error as DockerError,
    models::{ContainerInspectResponse, Volume},
    query_parameters::{RemoveContainerOptions, StopContainerOptions},
};
use serde::Serialize;
use sha2::{Digest, Sha256};

use crate::{
    docker_client::docker_deadline_after,
    error::HostError,
    journal::{ConnectionJournal, UnboundRetirementClaim, UnboundRetirementPhase},
    launch::SelectedEngineContext,
};

#[path = "historical_v48/initializer.rs"]
mod initializer;

const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);

pub(crate) async fn retire_before_settlement(
    docker: &Docker,
    scope: &ConnectionJournal,
    engine: &SelectedEngineContext,
    reservation_id: i64,
) -> Result<UnboundRetirementClaim, HostError> {
    scope
        .abandon_unbound_pair_v48_for_test(reservation_id)
        .await?;
    let Some(claim) = scope
        .claim_unbound_retirement_v48_for_test(reservation_id)
        .await?
    else {
        return Err(HostError::Journal);
    };
    let effects = Effects {
        docker,
        scope,
        engine,
        claim: &claim,
    };
    if claim.engine_id() != engine.engine_id()
        || claim.docker_root_digest() != engine.docker_root_digest()
    {
        return Err(HostError::Ownership);
    }
    for phase in [
        UnboundRetirementPhase::ContainersAbsent,
        UnboundRetirementPhase::VolumesAbsent,
    ] {
        if scope
            .unbound_retirement_phase_recorded_v48_for_test(&claim, phase)
            .await?
        {
            continue;
        }
        let evidence = match phase {
            UnboundRetirementPhase::ContainersAbsent => {
                Box::pin(effects.remove_containers()).await?
            }
            UnboundRetirementPhase::VolumesAbsent => Box::pin(effects.remove_volumes()).await?,
        };
        let digest =
            Sha256::digest(serde_json::to_vec(&evidence).map_err(|_| HostError::Identity)?)
                .iter()
                .flat_map(|byte| [byte >> 4, byte & 0x0f])
                .map(|nibble| char::from(b"0123456789abcdef"[usize::from(nibble)]))
                .collect::<String>();
        scope
            .record_unbound_retirement_phase_v48_for_test(&claim, phase, &digest)
            .await?;
    }
    Box::pin(super::super::containers::remove_unbound_v48_for_test(
        docker, scope, engine, &claim,
    ))
    .await?;
    Ok(claim)
}

#[derive(Serialize)]
struct Observation {
    role: String,
    name: String,
    id: Option<String>,
    outcome: &'static str,
}

struct Effects<'a> {
    docker: &'a Docker,
    scope: &'a ConnectionJournal,
    engine: &'a SelectedEngineContext,
    claim: &'a UnboundRetirementClaim,
}

impl Effects<'_> {
    async fn authorized<T, F>(&self, effect: F) -> Result<T, HostError>
    where
        F: Future<Output = Result<T, HostError>>,
    {
        self.scope
            .renew_unbound_retirement_claim_v48_for_test(self.claim)
            .await?;
        self.verify_engine().await?;
        effect.await
    }

    async fn verify_engine(&self) -> Result<(), HostError> {
        let selected = crate::journal::EngineContext {
            engine_id: self.engine.engine_id().to_owned(),
            endpoint: self.engine.endpoint().to_owned(),
            platform: self.engine.platform().to_owned(),
        };
        let info = within(self.docker.info())
            .await?
            .map_err(|_| HostError::Docker)?;
        let current = SelectedEngineContext::capture(&selected, &info)?;
        if current.matches_selection(&selected)
            && current.engine_id() == self.claim.engine_id()
            && current.docker_root_digest() == self.claim.docker_root_digest()
        {
            Ok(())
        } else {
            Err(HostError::Ownership)
        }
    }

    async fn remove_containers(&self) -> Result<Vec<Observation>, HostError> {
        let mut observations = Vec::new();
        for container in self
            .scope
            .unbound_inventory_v48_for_test(self.claim)
            .await?
            .containers
        {
            let id = container.id.as_deref().ok_or(HostError::Journal)?;
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

    async fn remove_volumes(&self) -> Result<Vec<Observation>, HostError> {
        let mut observations = Vec::new();
        for volume in self
            .scope
            .unbound_inventory_v48_for_test(self.claim)
            .await?
            .volumes
        {
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

    fn validate_container(
        &self,
        container: &ContainerInspectResponse,
        role: &str,
        name: &str,
        id: &str,
    ) -> Result<(), HostError> {
        let labels = container
            .config
            .as_ref()
            .and_then(|config| config.labels.as_ref())
            .ok_or(HostError::Ownership)?;
        let expected = HashMap::from([
            ("io.velnor.managed", "true".to_owned()),
            (
                "io.velnor.reservation-id",
                self.claim.reservation_id().to_string(),
            ),
            (
                "io.velnor.operation-id",
                self.claim.operation_id().to_string(),
            ),
            (
                "io.velnor.connection-id",
                self.claim.connection_id().to_owned(),
            ),
            ("io.velnor.role", role.to_owned()),
        ]);
        if container.id.as_deref() != Some(id)
            || container.name.as_deref() != Some(format!("/{name}").as_str())
            || expected
                .iter()
                .any(|(key, value)| labels.get(*key) != Some(value))
        {
            return Err(HostError::Ownership);
        }
        Ok(())
    }

    fn validate_volume(&self, volume: &Volume, role: &str, name: &str) -> Result<(), HostError> {
        let expected = HashMap::from([
            ("velnor.connection", self.claim.connection_id().to_owned()),
            (
                "velnor.reservation",
                self.claim.reservation_id().to_string(),
            ),
            ("velnor.operation", self.claim.operation_id().to_string()),
            ("velnor.role", role.to_owned()),
        ]);
        if volume.name != name
            || volume.driver != "local"
            || volume.labels.len() != expected.len()
            || expected
                .iter()
                .any(|(key, value)| volume.labels.get(*key) != Some(value))
        {
            return Err(HostError::Ownership);
        }
        Ok(())
    }
}

fn observation(role: &str, name: &str, id: Option<&str>, outcome: &'static str) -> Observation {
    Observation {
        role: role.to_owned(),
        name: name.to_owned(),
        id: id.map(str::to_owned),
        outcome,
    }
}

async fn inspect_container(
    docker: &Docker,
    name_or_id: &str,
) -> Result<Option<ContainerInspectResponse>, HostError> {
    match within(docker.inspect_container(name_or_id, None)).await? {
        Ok(value) => Ok(Some(value)),
        Err(DockerError::DockerResponseServerError {
            status_code: 404, ..
        }) => Ok(None),
        Err(_) => Err(HostError::Docker),
    }
}

async fn inspect_volume(docker: &Docker, name: &str) -> Result<Option<Volume>, HostError> {
    match within(docker.inspect_volume(name)).await? {
        Ok(volume) => Ok(Some(volume)),
        Err(DockerError::DockerResponseServerError {
            status_code: 404, ..
        }) => Ok(None),
        Err(_) => Err(HostError::Docker),
    }
}

async fn within<F: Future>(future: F) -> Result<F::Output, HostError> {
    docker_deadline_after(future, REQUEST_TIMEOUT).await
}

pub(crate) async fn settle(
    scope: &ConnectionJournal,
    claim: &UnboundRetirementClaim,
) -> Result<(), HostError> {
    scope.settle_unbound_retirement_v48_for_test(claim).await
}
