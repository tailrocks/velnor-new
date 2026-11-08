// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::time::{Duration, Instant};

use bollard::Docker;
use bollard::models::{ContainerCreateBody, HostConfig, Mount, MountType};
use bollard::query_parameters::StartContainerOptions;

use super::mutation_lifecycle::{self, MutationKind, MutationResult, MutationValue};
use super::resources::OwnedResources;
use super::{create_options, deadline, read_logs, remaining, wait_success};
use crate::error::HostError;
use crate::resource_probe_record::ProbeRecord;

pub(super) async fn qualify(
    docker: &Docker,
    image: &str,
    root: &str,
    healthcheck: Option<bollard::models::HealthConfig>,
    end: Instant,
) -> Result<(), HostError> {
    let mut resources = OwnedResources::new(&[(image, healthcheck)])?;
    let result = match remaining(end) {
        Ok(limit) => deadline(run(docker, image, root, &mut resources), limit)
            .await
            .and_then(std::convert::identity),
        Err(error) => Err(error),
    };
    let cleanup = resources.cleanup(docker).await;
    match (result, cleanup) {
        (Ok(()), Ok(())) => Ok(()),
        (Err(error), Ok(())) | (_, Err(error)) => Err(error),
    }
}

async fn run(
    docker: &Docker,
    image: &str,
    root: &str,
    resources: &mut OwnedResources,
) -> Result<(), HostError> {
    if !root.starts_with('/') || root.chars().any(char::is_control) {
        return Err(HostError::Identity);
    }
    let end = Instant::now() + Duration::from_secs(30);
    let name = resources.register_container("resource-probe")?;
    let config = probe_config(resources, image, root);
    let client = docker.clone();
    let options = create_options(&name);
    let mutation = mutation_lifecycle::spawn(
        format!("container {name}"),
        MutationKind::Container(name.clone()),
        async move {
            let created = match client.create_container(Some(options), config).await {
                Ok(created) => created,
                Err(error) => return mutation_lifecycle::uncertain_docker(&error),
            };
            if super::valid_container_id(&created.id).is_err() {
                return mutation_lifecycle::uncertain(HostError::Identity);
            }
            MutationResult::Created(MutationValue::ContainerId(created.id))
        },
    );
    let id = match resources.await_mutation(mutation, remaining(end)?).await? {
        MutationValue::ContainerId(id) => id,
        MutationValue::Volume => return Err(HostError::Identity),
    };
    resources.bind_container_id(&name, &id)?;
    let verify = async {
        let inspect = deadline(docker.inspect_container(&id, None), remaining(end)?)
            .await?
            .map_err(|_| HostError::Docker)?;
        resources.validate_inspect(&inspect, &name, &id, "resource-probe", image, &[root])?;
        Ok::<_, HostError>(inspect)
    };
    verify.await?;
    deadline(
        docker.start_container(&id, None::<StartContainerOptions>),
        remaining(end)?,
    )
    .await?
    .map_err(|_| HostError::Docker)?;
    wait_success(docker, &id, remaining(end)?).await?;
    let output = read_logs(docker, &id, remaining(end)?).await?;
    ProbeRecord::parse_schema5(&output).ok_or(HostError::Identity)?;
    let inspect = deadline(docker.inspect_container(&id, None), remaining(end)?)
        .await?
        .map_err(|_| HostError::Docker)?;
    resources.validate_inspect(&inspect, &name, &id, "resource-probe", image, &[root])?;
    if inspect.state.and_then(|state| state.exit_code) != Some(0) {
        return Err(HostError::Docker);
    }
    Ok(())
}

fn probe_config(resources: &OwnedResources, image: &str, root: &str) -> ContainerCreateBody {
    ContainerCreateBody {
        image: Some(image.to_owned()),
        user: Some("65532:65532".to_owned()),
        entrypoint: Some(vec!["/velnor/resource-probe".to_owned()]),
        attach_stdout: Some(true),
        attach_stderr: Some(true),
        tty: Some(false),
        labels: Some(resources.labels("resource-probe")),
        host_config: Some(HostConfig {
            privileged: Some(false),
            network_mode: Some("none".to_owned()),
            readonly_rootfs: Some(true),
            pid_mode: Some("host".to_owned()),
            cgroupns_mode: Some(bollard::models::HostConfigCgroupnsModeEnum::HOST),
            cap_drop: Some(vec!["ALL".to_owned()]),
            security_opt: Some(vec!["no-new-privileges:true".to_owned()]),
            mounts: Some(vec![Mount {
                typ: Some(MountType::BIND),
                source: Some(root.to_owned()),
                target: Some("/velnor/docker-root".to_owned()),
                read_only: Some(true),
                ..Default::default()
            }]),
            auto_remove: Some(false),
            ..Default::default()
        }),
        ..Default::default()
    }
}
