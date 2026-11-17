// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use bollard::models::{ContainerInspectResponse, NetworkInspect, Volume};
use serde::Serialize;

use crate::{
    error::HostError,
    journal::{StorageNetworkRecoveryIdentity, UnboundResourceKind, UnboundRetirementClaim},
};

use super::Effects;

#[derive(Debug)]
pub(crate) struct UnboundCreationObservation {
    kind: UnboundResourceKind,
    identity: Identity,
    json: String,
    digest: String,
}

#[derive(Debug, Serialize)]
struct Identity {
    resource_kind: &'static str,
    role: String,
    docker_name: String,
    docker_id: String,
    reservation_id: i64,
    connection_id: String,
    operation_id: i64,
    engine_id: String,
    docker_root_digest: String,
    driver: Option<String>,
    labels: BTreeMap<String, String>,
}

impl UnboundCreationObservation {
    pub(super) fn container(
        effects: &Effects<'_>,
        role: &str,
        name: &str,
        inspected: &ContainerInspectResponse,
    ) -> Result<Self, HostError> {
        let id = inspected
            .id
            .as_deref()
            .filter(|id| !id.is_empty())
            .ok_or(HostError::Ownership)?;
        effects.validate_container(inspected, role, name, id)?;
        let labels = inspected
            .config
            .as_ref()
            .and_then(|config| config.labels.as_ref())
            .ok_or(HostError::Ownership)?;
        Self::new(
            effects.claim,
            UnboundResourceKind::Container,
            role,
            name,
            id,
            None,
            labels
                .iter()
                .filter(|(key, _)| {
                    matches!(
                        key.as_str(),
                        "io.velnor.managed"
                            | "io.velnor.reservation-id"
                            | "io.velnor.operation-id"
                            | "io.velnor.connection-id"
                            | "io.velnor.role"
                    )
                })
                .map(|(key, value)| (key.clone(), value.clone()))
                .collect(),
        )
    }

    pub(super) fn volume(
        effects: &Effects<'_>,
        role: &str,
        name: &str,
        inspected: &Volume,
    ) -> Result<Self, HostError> {
        effects.validate_volume(inspected, role, name)?;
        Self::new(
            effects.claim,
            UnboundResourceKind::Volume,
            role,
            name,
            &inspected.name,
            Some(inspected.driver.clone()),
            inspected
                .labels
                .iter()
                .map(|(key, value)| (key.clone(), value.clone()))
                .collect(),
        )
    }

    pub(super) fn network(
        effects: &Effects<'_>,
        expected: &StorageNetworkRecoveryIdentity,
        inspected: &NetworkInspect,
    ) -> Result<Self, HostError> {
        if expected.reservation_id() != effects.claim.reservation_id()
            || expected.operation_id() != effects.claim.operation_id()
            || expected.engine_id() != effects.claim.engine_id()
            || expected.docker_root_digest() != effects.claim.docker_root_digest()
        {
            return Err(HostError::Ownership);
        }
        let id = inspected.id.as_deref().ok_or(HostError::Ownership)?;
        super::super::containers::validate_pre_jit_network(
            inspected,
            expected.network_name(),
            id,
            expected.labels(),
        )?;
        Self::new(
            effects.claim,
            UnboundResourceKind::Network,
            "private",
            expected.network_name(),
            id,
            None,
            expected.labels().clone(),
        )
    }

    fn new(
        claim: &UnboundRetirementClaim,
        kind: UnboundResourceKind,
        role: &str,
        name: &str,
        id: &str,
        driver: Option<String>,
        labels: BTreeMap<String, String>,
    ) -> Result<Self, HostError> {
        let identity = Identity {
            resource_kind: kind.as_str(),
            role: role.to_owned(),
            docker_name: name.to_owned(),
            docker_id: id.to_owned(),
            reservation_id: claim.reservation_id(),
            connection_id: claim.connection_id().to_owned(),
            operation_id: claim.operation_id(),
            engine_id: claim.engine_id().to_owned(),
            docker_root_digest: claim.docker_root_digest().to_owned(),
            driver,
            labels,
        };
        let json = serde_json::to_string(&identity).map_err(|_| HostError::Identity)?;
        let digest = super::digest(&identity)?;
        Ok(Self {
            kind,
            identity,
            json,
            digest,
        })
    }

    pub(crate) const fn resource_kind(&self) -> UnboundResourceKind {
        self.kind
    }
    pub(crate) fn role(&self) -> &str {
        &self.identity.role
    }
    pub(crate) fn docker_name(&self) -> &str {
        &self.identity.docker_name
    }
    pub(crate) fn docker_id(&self) -> &str {
        &self.identity.docker_id
    }
    pub(crate) fn observation_sha256(&self) -> &str {
        &self.digest
    }
    pub(crate) fn observation_json(&self) -> &str {
        &self.json
    }
    pub(crate) const fn reservation_id(&self) -> i64 {
        self.identity.reservation_id
    }
    pub(crate) fn connection_id(&self) -> &str {
        &self.identity.connection_id
    }
    pub(crate) const fn operation_id(&self) -> i64 {
        self.identity.operation_id
    }
    pub(crate) fn engine_id(&self) -> &str {
        &self.identity.engine_id
    }
    pub(crate) fn docker_root_digest(&self) -> &str {
        &self.identity.docker_root_digest
    }
}
