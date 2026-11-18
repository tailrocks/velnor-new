// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use bollard::{
    models::ContainerInspectResponse,
    query_parameters::{RemoveContainerOptions, StopContainerOptions},
};

use crate::{error::HostError, journal::UnboundInitializer};

use super::{Observation, ReadyEffects, inspect_container, observation, within};

impl ReadyEffects<'_> {
    pub(super) async fn remove_initializer(
        &self,
        helper: &UnboundInitializer,
    ) -> Result<Observation, HostError> {
        let by_name = self
            .authorized(inspect_container(self.docker, &helper.name))
            .await?;
        let by_id = match helper.id.as_deref() {
            Some(id) => self.authorized(inspect_container(self.docker, id)).await?,
            None => None,
        };
        let id = match plan_initializer_removal(helper, by_name.as_ref(), by_id.as_ref())? {
            InitializerPlan::Absent => {
                return Ok(observation(
                    "storage-initializer",
                    &helper.name,
                    helper.id.as_deref(),
                    "already_absent",
                ));
            }
            InitializerPlan::Remove { id } => id,
        };
        let inspected = by_name.ok_or(HostError::Ownership)?;
        if inspected.state.as_ref().and_then(|state| state.running) == Some(true) {
            let stop = self.docker.stop_container(
                &id,
                Some(StopContainerOptions {
                    t: Some(1),
                    signal: None,
                }),
            );
            Box::pin(self.authorized(async { within(stop).await?.map_err(|_| HostError::Docker) }))
                .await?;
        }
        let remove = self.docker.remove_container(
            &id,
            Some(RemoveContainerOptions {
                force: false,
                v: false,
                link: false,
            }),
        );
        Box::pin(self.authorized(async { within(remove).await?.map_err(|_| HostError::Docker) }))
            .await?;
        if self
            .authorized(inspect_container(self.docker, &id))
            .await?
            .is_some()
            || self
                .authorized(inspect_container(self.docker, &helper.name))
                .await?
                .is_some()
        {
            return Err(HostError::Ownership);
        }
        Ok(observation(
            "storage-initializer",
            &helper.name,
            Some(&id),
            "removed_and_absent",
        ))
    }
}

pub(crate) enum InitializerPlan {
    Absent,
    Remove { id: String },
}

pub(crate) fn plan_initializer_removal(
    helper: &UnboundInitializer,
    by_name: Option<&ContainerInspectResponse>,
    by_id: Option<&ContainerInspectResponse>,
) -> Result<InitializerPlan, HostError> {
    let Some(inspected) = by_name else {
        if by_id.is_some() {
            return Err(HostError::Ownership);
        }
        return Ok(InitializerPlan::Absent);
    };
    let id = inspected
        .id
        .clone()
        .filter(|id| !id.is_empty())
        .ok_or(HostError::Ownership)?;
    if helper.id.as_deref().is_some_and(|saved| saved != id) {
        return Err(HostError::Ownership);
    }
    validate_initializer(inspected, helper)?;
    Ok(InitializerPlan::Remove { id })
}

fn validate_initializer(
    inspected: &ContainerInspectResponse,
    helper: &UnboundInitializer,
) -> Result<(), HostError> {
    let labels = inspected
        .config
        .as_ref()
        .and_then(|config| config.labels.as_ref())
        .ok_or(HostError::Ownership)?;
    if inspected.name.as_deref() != Some(format!("/{}", helper.name).as_str())
        || labels.get("velnor.role").map(String::as_str) != Some("storage-initializer")
        || labels.get("velnor.operation") != Some(&helper.operation_key)
    {
        return Err(HostError::Ownership);
    }
    Ok(())
}
