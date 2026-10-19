//! Docker create projection for one runner and its private `DinD`.
//!
//! JIT is not a field. `start_pair` writes it on stdin and does not store it.

use bollard::Docker;
use bollard::models::ContainerCreateBody;
use bollard::query_parameters::CreateContainerOptions;
use bollard::query_parameters::{AttachContainerOptionsBuilder, StartContainerOptions};
use tokio::io::AsyncWriteExt;

use crate::docker_client::docker_deadline;
use crate::docker_spec::{ContainerPlan, Mount, audit_plan, runner_mounts, runner_plan};
use crate::error::HostError;

mod names;
pub(crate) mod resources;
mod volumes;
pub(crate) use names::{WorkerContainerRole, WorkerIdentityNames};
#[cfg(test)]
pub(crate) use volumes::remove_verified_worker_volume;
pub(crate) use volumes::{
    VerifiedWorkerVolume, WorkerVolumeRemoval, WorkerVolumeRole, WorkerVolumeVerification,
    create_named_volumes, remove_worker_volumes, verify_worker_volume,
};
pub(crate) mod guest_image;
mod mounts;
mod projection;
mod projection_types;
mod resource_budget;
#[cfg(all(test, unix))]
mod volumes_tests;
#[cfg(test)]
pub(crate) use projection::{dind_create_for_identity, runner_create_for_identity};
#[cfg(test)]
pub(crate) use projection::{identity_labels_match, launch_identity_labels_match};
pub use resource_budget::ResourceBudget;
pub(crate) use resource_budget::ResourceBudgetConfig;
#[cfg(test)]
pub(crate) use resource_budget::test_resource_budget;
pub(crate) use resource_budget::{
    AdmissionReserve, DockerResourceLimits, PROBE_CPU_MILLICORES, PROBE_MEMORY_LIMIT_BYTES,
};
#[cfg(test)]
mod projection_tests;

const PLATFORM: &str = "linux/amd64";
const DIND_IMAGE: &str = "velnor-dind:29.9.1";
#[cfg(test)]
const DIND_ENTRYPOINT: [&str; 1] = ["/usr/local/bin/velnor-dind-entrypoint"];
const IDENTITY_HEX: &[u8; 16] = b"0123456789abcdef";

pub use projection_types::{BindMount, BollardCreate, CreateProjection, Started};

/// Generate a collision-resistant worker volume base for one journal row.
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

/// Make a Docker create request only after `audit_plan` gives success.
/// The caller writes the JIT configuration to stdin.
/// The plan does not contain it.
///
/// # Errors
///
/// Gives [`HostError::PrivilegedRunner`] or [`HostError::ForbiddenMount`] when
/// [`audit_plan`] fails or when `env`, `cmd`, or labels contain JIT.
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

/// Make a runner Docker create request only after
/// the release and selected-engine checks give success.
/// Use the image IDs in the selected engine.
/// Use the guest metadata file for the same connection and image bundle.
pub(crate) fn runner_create_verified(
    plan: &ContainerPlan,
    resource_budget: ResourceBudget,
    connection: &crate::config::RepositoryConnection,
    images: &crate::launch::VerifiedWorkerImages,
    guest_image: &guest_image::GuestImageFile,
) -> Result<CreateProjection, HostError> {
    let metadata_mount = guest_image.runner_bind(connection, images)?;
    let mut projection = runner_create(plan, resource_budget)?;
    projection.image = images.runner().runtime_id().to_owned();
    projection.platform = images.platform().to_owned();
    projection.bind_mounts.push(metadata_mount);
    Ok(projection)
}

/// Join `spec` to the private `DinD` network namespace.
///
/// Published service ports and Testcontainers use the runner's localhost.
/// `dind_id` must identify one Docker container.
/// The function sets the network mode to the Docker-in-Docker container.
///
/// # Errors
///
/// Gives [`HostError::ForbiddenMount`] when `dind_id` is not a 64-character string of hexadecimal digits.
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

/// Make a privileged Docker-in-Docker container request.
/// Do not set this privilege on the runner plan.
///
/// The runner plan's socket volume is at `/run`.
/// The shared Home volume is at `/home/runner`.
/// The shared work volume is at `/home/runner/_work`.
/// A DinD-only Docker volume is at `/var/lib/docker`.
/// The runner container does not mount the Docker data volume.
/// The Docker-in-Docker container does not mount the action archive volume.
/// The Docker data volume uses storage outside the container layer.
///
/// # Errors
///
/// Gives [`HostError::ForbiddenMount`] when `private_volume` does not name one correct private volume.
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

/// Make a privileged Docker-in-Docker request only after release and selected engine checks for both images pass.
pub(crate) fn dind_create_verified(
    private_volume: &str,
    resource_budget: ResourceBudget,
    images: &crate::launch::VerifiedWorkerImages,
) -> Result<CreateProjection, HostError> {
    guest_image::validate_worker_images(images)?;
    let mut projection = dind_create(private_volume, resource_budget)?;
    projection.image = images.dind().runtime_id().to_owned();
    projection.platform = images.platform().to_owned();
    Ok(projection)
}

pub(crate) fn dind_mounts(private_volume: &str) -> Result<Vec<Mount>, HostError> {
    volumes::mounts_for_dind(private_volume)
}

/// Stable `DinD` container name used by cleanup and recovery.
///
/// This function does not use a resource budget.
/// Cleanup does not use the current resource budget.
pub(crate) fn dind_container_name(private_volume: &str) -> Result<String, HostError> {
    runner_plan(private_volume)?;
    Ok(format!("{private_volume}-dind"))
}

/// Bollard config for [`bollard::Docker::create_container`]. No JIT parameter.
///
/// # Errors
///
/// Gives [`HostError::ForbiddenMount`] when the platform is not `linux/amd64`
/// or a mount or label cannot be sent.
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
