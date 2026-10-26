// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use bollard::Docker;
use bollard::models::ContainerCreateBody;
use bollard::query_parameters::CreateContainerOptions;
use bollard::query_parameters::{AttachContainerOptionsBuilder, StartContainerOptions};
use tokio::io::AsyncWriteExt;

use crate::docker_client::docker_deadline;
use crate::docker_spec::{ContainerPlan, Mount, audit_plan, runner_mounts, runner_plan};
use crate::error::HostError;
use crate::stage::PairStop;

pub(crate) mod resources;
mod volumes;
pub(crate) use volumes::{
    VerifiedWorkerVolume, WorkerVolumeRemoval, WorkerVolumeRole, WorkerVolumeVerification,
    create_named_volumes, remove_worker_volumes, verify_worker_volume,
};
mod mounts;
mod projection;
mod projection_types;
mod resource_budget;

pub(crate) use resource_budget::DockerResourceLimits;
pub use resource_budget::ResourceBudget;
pub(crate) use resource_budget::ResourceBudgetConfig;

const PLATFORM: &str = "linux/amd64";
const DIND_IMAGE: &str = "velnor-dind:29.9.1";
const IDENTITY_HEX: &[u8; 16] = b"0123456789abcdef";

pub use projection_types::{BindMount, BollardCreate, CreateProjection, Started};

pub(crate) fn new_worker_volume() -> Result<String, HostError> {
    let mut random = [0_u8; 16];
    getrandom::fill(&mut random).map_err(|_| HostError::Identity)?;
    let mut name = String::with_capacity(33);
    name.push('w');
    for byte in random {
        push_nibble(&mut name, byte >> 4)?;
        push_nibble(&mut name, byte & 0x0f)?;
    }
    Ok(name)
}

fn push_nibble(name: &mut String, nibble: u8) -> Result<(), HostError> {
    let index = usize::from(nibble);
    let digit = IDENTITY_HEX
        .get(index)
        .copied()
        .ok_or(HostError::Identity)?;
    name.push(char::from(digit));
    Ok(())
}

pub fn runner_create(
    plan: &ContainerPlan,
    resource_budget: ResourceBudget,
) -> Result<CreateProjection, HostError> {
    audit_plan(plan)?;
    if plan.cmd.iter().any(|item| cmd_names_jit(item)) {
        return Err(HostError::ForbiddenMount);
    }
    Ok(CreateProjection {
        name: plan.name.clone(),
        image: plan.image.clone(),
        platform: plan.platform.clone(),
        env: plan.env.clone(),
        cmd: plan.cmd.clone(),
        entrypoint: Vec::new(),
        user: None,
        working_dir: None,
        labels: plan.labels.clone(),
        mounts: runner_mounts(&plan.mounts)?,
        bind_mounts: Vec::new(),
        privileged: false,
        open_stdin: true,
        network_mode: None,
        resource_budget,
    })
}

pub(crate) fn join_dind_net(
    mut spec: CreateProjection,
    dind_id: &str,
) -> Result<CreateProjection, HostError> {
    if !dind_container_id(dind_id) {
        return Err(HostError::ForbiddenMount);
    }
    spec.network_mode = Some(format!("container:{dind_id}"));
    Ok(spec)
}

fn dind_container_id(id: &str) -> bool {
    id.len() == 64 && id.bytes().all(|byte| byte.is_ascii_hexdigit())
}

pub fn dind_create(
    private_volume: &str,
    resource_budget: ResourceBudget,
) -> Result<CreateProjection, HostError> {
    let runner = runner_plan(private_volume)?;
    let mounts = dind_mounts_for(private_volume, runner.mounts);
    let mut labels = vec![
        "velnor.role=dind".to_owned(),
        format!("velnor.volume={private_volume}"),
        format!("velnor.worker={private_volume}"),
    ];
    labels.sort_unstable();
    Ok(CreateProjection {
        name: format!("{private_volume}-dind"),
        image: DIND_IMAGE.to_owned(),
        platform: runner.platform,
        env: Vec::new(),
        cmd: Vec::new(),
        entrypoint: Vec::new(),
        user: None,
        working_dir: None,
        labels,
        mounts,
        bind_mounts: Vec::new(),
        privileged: true,
        open_stdin: false,
        network_mode: None,
        resource_budget,
    })
}

pub(crate) fn dind_mounts(private_volume: &str) -> Result<Vec<Mount>, HostError> {
    let runner = runner_plan(private_volume)?;
    Ok(dind_mounts_for(private_volume, runner.mounts))
}

fn dind_mounts_for(private_volume: &str, mut mounts: Vec<Mount>) -> Vec<Mount> {
    mounts.push(Mount {
        source: format!("volume:{private_volume}-docker"),
        target: "/var/lib/docker".to_owned(),
    });
    mounts
}

pub(crate) fn dind_container_name(private_volume: &str) -> Result<String, HostError> {
    runner_plan(private_volume)?;
    Ok(format!("{private_volume}-dind"))
}

pub fn bollard_create(spec: &CreateProjection) -> Result<BollardCreate, HostError> {
    if spec.platform != PLATFORM {
        return Err(HostError::ForbiddenMount);
    }
    let config = ContainerCreateBody {
        image: Some(spec.image.clone()),
        env: none_if_empty(&spec.env),
        cmd: none_if_empty(&spec.cmd),
        labels: mounts::label_map(&spec.labels)?,
        open_stdin: Some(spec.open_stdin),
        attach_stdin: Some(spec.open_stdin),
        stdin_once: Some(spec.open_stdin),
        host_config: Some(mounts::host_config(spec)?),
        ..Default::default()
    };
    Ok(BollardCreate {
        options: CreateContainerOptions {
            name: Some(spec.name.clone()),
            platform: PLATFORM.to_owned(),
        },
        config,
    })
}

pub async fn start_pair(
    docker: &Docker,
    private_volume: &str,
    resource_budget: ResourceBudget,
    jit: &[u8],
) -> Result<Started, HostError> {
    let partial = Box::pin(crate::stage::start_pair_until(
        docker,
        private_volume,
        resource_budget,
        jit,
        PairStop::Jit,
    ))
    .await?;
    Ok(Started {
        dind_id: partial.dind_id.ok_or(HostError::Docker)?,
        runner_id: partial.runner_id.ok_or(HostError::Docker)?,
    })
}

fn cmd_names_jit(item: &str) -> bool {
    item.to_ascii_lowercase().contains("jit")
}

fn none_if_empty(items: &[String]) -> Option<Vec<String>> {
    if items.is_empty() {
        None
    } else {
        Some(items.to_vec())
    }
}

pub(crate) async fn worker_id_for_name(
    docker: &Docker,
    name: &str,
    volume: &str,
    role: &str,
) -> Result<Option<String>, HostError> {
    if name.is_empty() || volume.is_empty() || role.is_empty() {
        return Err(HostError::Docker);
    }
    match Box::pin(docker_deadline(docker.inspect_container(name, None))).await? {
        Ok(body) => {
            let id = body
                .id
                .filter(|id| !id.is_empty())
                .ok_or(HostError::Docker)?;
            let labels = body
                .config
                .and_then(|config| config.labels)
                .ok_or(HostError::Docker)?;
            if labels.get("velnor.volume").map(String::as_str) != Some(volume)
                || labels.get("velnor.worker").map(String::as_str) != Some(volume)
                || labels.get("velnor.role").map(String::as_str) != Some(role)
            {
                return Err(HostError::Docker);
            }
            Ok(Some(id))
        }
        Err(bollard::errors::Error::DockerResponseServerError {
            status_code: 404, ..
        }) => Ok(None),
        Err(_) => Err(HostError::Docker),
    }
}

pub(crate) async fn create_only(
    docker: &Docker,
    spec: &CreateProjection,
) -> Result<String, HostError> {
    let created = bollard_create(spec)?;
    let response = docker
        .create_container(Some(created.options), created.config)
        .await
        .map_err(|_| HostError::Docker)?;
    if response.id.is_empty() {
        Err(HostError::Docker)
    } else {
        Ok(response.id)
    }
}

pub(crate) async fn start_id(docker: &Docker, id: &str) -> Result<(), HostError> {
    docker
        .start_container(id, None::<StartContainerOptions>)
        .await
        .map_err(|_| HostError::Docker)
}

pub(crate) async fn deliver_jit(docker: &Docker, id: &str, jit: &[u8]) -> Result<(), HostError> {
    let options = AttachContainerOptionsBuilder::new()
        .stdin(true)
        .stream(true)
        .build();
    let mut attached = docker
        .attach_container(id, Some(options))
        .await
        .map_err(|_| HostError::Docker)?;
    attached
        .input
        .write_all(jit)
        .await
        .map_err(|_| HostError::Docker)?;
    attached
        .input
        .shutdown()
        .await
        .map_err(|_| HostError::Docker)?;
    Ok(())
}
