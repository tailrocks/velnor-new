// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::HashMap;
use std::future::Future;
use std::time::Duration;

use bollard::{
    Docker,
    errors::Error as DockerError,
    models::{ContainerInspectResponse, Volume},
};
use serde::Serialize;
use sha2::{Digest, Sha256};

use crate::{
    docker_client::docker_deadline_after,
    error::HostError,
    journal::{ConnectionJournal, UnboundRetirementClaim, UnboundRetirementPhase},
    launch::SelectedEngineContext,
};

#[cfg(test)]
#[path = "unbound_retirement/historical_v48.rs"]
pub(crate) mod historical_v48;

#[path = "unbound_retirement/creation_target.rs"]
mod creation_target;
pub(crate) use creation_target::UnboundCreationObservation;

#[path = "unbound_retirement/bounded.rs"]
mod bounded;
#[path = "unbound_retirement/effects.rs"]
mod effects;
#[path = "unbound_retirement/initializer.rs"]
mod initializer;
#[path = "unbound_retirement/preflight.rs"]
mod preflight;

pub(crate) use bounded::{
    abandon_and_retire_unbound_pair_before, retire_unbound_pair_before,
    retire_uncertain_unbound_pairs,
};
pub(crate) use initializer::{InitializerPlan, plan_initializer_removal};

#[cfg(test)]
#[path = "unbound_retirement/tests.rs"]
pub(crate) mod tests;

const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);

async fn abandon_and_retire_unbound_pair(
    docker: &Docker,
    scope: &ConnectionJournal,
    engine: &SelectedEngineContext,
    reservation_id: i64,
) -> Result<bool, HostError> {
    scope.abandon_unbound_pair(reservation_id).await?;
    Box::pin(retire_unbound_pair(docker, scope, engine, reservation_id)).await
}

pub(crate) async fn retire_unbound_pair(
    docker: &Docker,
    scope: &ConnectionJournal,
    engine: &SelectedEngineContext,
    reservation_id: i64,
) -> Result<bool, HostError> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|_| HostError::Journal)?
        .as_secs();
    retire_unbound_pair_at(
        docker,
        scope,
        engine,
        reservation_id,
        i64::try_from(now).map_err(|_| HostError::Journal)?,
    )
    .await
}

pub(crate) async fn retire_unbound_pair_at(
    docker: &Docker,
    scope: &ConnectionJournal,
    engine: &SelectedEngineContext,
    reservation_id: i64,
    now: i64,
) -> Result<bool, HostError> {
    let Some(claim) = scope.claim_unbound_retirement(reservation_id).await? else {
        return Ok(false);
    };
    let effects = Effects {
        docker,
        scope,
        engine,
        claim: &claim,
        now,
    };
    if claim.engine_id() != engine.engine_id()
        || claim.docker_root_digest() != engine.docker_root_digest()
    {
        return Err(HostError::Ownership);
    }
    let effects = match effects.preflight().await? {
        preflight::Publication::Ready(effects) => effects,
        preflight::Publication::Deferred => {
            scope.defer_unbound_retirement_claim(&claim).await?;
            return Ok(false);
        }
    };
    for phase in [
        UnboundRetirementPhase::ContainersAbsent,
        UnboundRetirementPhase::VolumesAbsent,
    ] {
        if scope
            .unbound_retirement_phase_recorded(&claim, phase)
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
        let digest = digest(&evidence)?;
        scope
            .record_unbound_retirement_phase(&claim, phase, &digest)
            .await?;
    }
    Box::pin(super::remove_owned_network_for_unbound_retirement(
        docker, scope, engine, &claim,
    ))
    .await?;
    scope.settle_unbound_retirement(&claim).await?;
    Ok(true)
}

#[derive(Serialize)]
struct Observation {
    role: String,
    name: String,
    id: Option<String>,
    outcome: &'static str,
}

struct ReadyEffects<'a> {
    effects: Effects<'a>,
    inventory: crate::journal::UnboundRetirementInventory,
}

impl<'a> std::ops::Deref for ReadyEffects<'a> {
    type Target = Effects<'a>;

    fn deref(&self) -> &Self::Target {
        &self.effects
    }
}

struct Effects<'a> {
    docker: &'a Docker,
    scope: &'a ConnectionJournal,
    engine: &'a SelectedEngineContext,
    claim: &'a UnboundRetirementClaim,
    now: i64,
}

impl Effects<'_> {
    async fn authorized<T, F>(&self, effect: F) -> Result<T, HostError>
    where
        F: Future<Output = Result<T, HostError>>,
    {
        self.scope
            .renew_unbound_retirement_claim(self.claim)
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

fn digest(value: &impl Serialize) -> Result<String, HostError> {
    Ok(
        Sha256::digest(serde_json::to_vec(value).map_err(|_| HostError::Identity)?)
            .iter()
            .flat_map(|byte| [byte >> 4, byte & 0x0f])
            .map(|nibble| char::from(b"0123456789abcdef"[usize::from(nibble)]))
            .collect(),
    )
}
