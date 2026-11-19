// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use bollard::models::ContainerCreateBody;
use bollard::query_parameters::CreateContainerOptions;

use crate::docker_spec::{ContainerPlan, audit_plan, runner_mounts, runner_plan};
use crate::error::HostError;

mod names;
pub(crate) mod resources;
mod volumes;
pub(crate) use names::{WorkerContainerRole, WorkerIdentityNames};
pub(crate) use volumes::WorkerVolumeRole;

pub(crate) mod execution_authorization;
pub(crate) mod guest_image;
mod mounts;
mod projection;
mod projection_types;
mod resource_budget;
pub(crate) mod seccomp;
pub(crate) mod storage_initialization;

pub use resource_budget::ResourceBudget;
pub(crate) use resource_budget::ResourceBudgetConfig;

pub(crate) use resource_budget::{
    AdmissionReserve, DockerResourceLimits, PROBE_CPU_MILLICORES, PROBE_MEMORY_LIMIT_BYTES,
};

const PLATFORM: &str = "linux/amd64";
const DIND_IMAGE: &str = "velnor-dind:29.9.1";

pub use execution_authorization::{
    ProofChannelBindingInput, ProofChannelMount, VerifiedProofChannelBinding,
};
pub use projection_types::{BindMount, BollardCreate, CreateProjection, Started};

pub fn runner_create(
    plan: &ContainerPlan,
    resource_budget: ResourceBudget,
) -> Result<CreateProjection, HostError> {
    audit_plan(plan)?;
    if plan.cmd.iter().any(|item| cmd_names_jit(item)) {
        return Err(HostError::ForbiddenMount);
    }
    let private_volume = plan
        .name
        .strip_suffix("-runner")
        .ok_or(HostError::ForbiddenMount)?;
    let planned_mounts = volumes::mounts_for_runner(private_volume)?;
    let plan_mounts = volumes::mounts_for_runner_plan(private_volume)?;
    if plan.mounts != plan_mounts {
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
        mounts: runner_mounts(&planned_mounts)?,
        bind_mounts: Vec::new(),
        privileged: false,
        open_stdin: true,
        network_mode: None,
        resource_budget,
    })
}

pub fn dind_create(
    private_volume: &str,
    resource_budget: ResourceBudget,
) -> Result<CreateProjection, HostError> {
    let runner = runner_plan(private_volume)?;
    let mounts = volumes::mounts_for_dind(private_volume)?;
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
