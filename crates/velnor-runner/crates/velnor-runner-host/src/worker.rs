//! Docker create projection for one runner and its private `DinD`.
//!
//! JIT is not a field. `start_pair` writes it on stdin and does not store it.

use bollard::Docker;
use bollard::models::ContainerCreateBody;
use bollard::query_parameters::{
    AttachContainerOptionsBuilder, CreateContainerOptions, StartContainerOptions,
};
use tokio::io::AsyncWriteExt;

use crate::docker_client::docker_deadline;
use crate::docker_spec::{ContainerPlan, Mount, audit_plan, runner_mounts, runner_plan};
use crate::error::HostError;
use crate::stage::PairStop;

pub(crate) mod resources;
mod volumes;
#[cfg(test)]
pub(crate) use volumes::remove_verified_worker_volume;
pub(crate) use volumes::{
    VerifiedWorkerVolume, WorkerVolumeRemoval, WorkerVolumeRole, WorkerVolumeVerification,
    create_named_volumes, remove_worker_volumes, verify_worker_volume,
};
mod mounts;
mod projection;
mod resource_budget;
#[cfg(all(test, unix))]
mod volumes_tests;
#[cfg(test)]
pub(crate) use projection::{dind_create_for_identity, runner_create_for_identity};
#[cfg(test)]
pub(crate) use projection::{identity_labels_match, launch_identity_labels_match};
#[cfg(test)]
pub(crate) use resource_budget::bounded_host_limits;
#[cfg(test)]
pub(crate) use resource_budget::test_resource_budget;
pub(crate) use resource_budget::{ResourceBudget, ResourceBudgetConfig};
#[cfg(test)]
mod projection_tests;

const PLATFORM: &str = "linux/amd64";
const DIND_IMAGE: &str = "velnor-dind:29.8.2";
const DIND_ENTRYPOINT: [&str; 1] = ["/usr/local/bin/velnor-dind-entrypoint"];
const IDENTITY_HEX: &[u8; 16] = b"0123456789abcdef";

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

/// One Docker create. Not a bollard type. JIT is not a field.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreateProjection {
    /// Deterministic per-worker container name used for crash recovery.
    pub name: String,
    /// Image reference.
    pub image: String,
    /// OCI platform. Always `linux/amd64`.
    pub platform: String,
    /// Env pairs. Empty when the image environment stands.
    pub env: Vec<String>,
    /// Command. Empty when the image entrypoint stands.
    pub cmd: Vec<String>,
    /// Expected image entrypoint. It is checked during reconciliation, not sent to Docker.
    pub entrypoint: Vec<String>,
    /// Expected image user. An empty image value means the image default.
    pub user: Option<String>,
    /// Expected image working directory. An empty image value means the image default.
    pub working_dir: Option<String>,
    /// `key=value` labels. No JIT.
    pub labels: Vec<String>,
    /// Mounts. Volume sources use `volume:<name>`.
    pub mounts: Vec<Mount>,
    /// Private host binds. The action archive bind is read-only.
    pub bind_mounts: Vec<BindMount>,
    /// Host privilege. False for the runner. True only for private `DinD`.
    pub privileged: bool,
    /// `OpenStdin`. True only for the runner channel.
    pub open_stdin: bool,
    /// `container:<id>` joins that container's network namespace. Runner only.
    pub network_mode: Option<String>,
    /// Validated CPU/memory budget. `None` keeps legacy unbounded behavior.
    pub(crate) resource_budget: Option<ResourceBudget>,
}

/// One controller-owned host bind in a runner create projection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BindMount {
    /// Controller-owned source path.
    pub source: String,
    /// Container path.
    pub target: String,
    /// Whether the container may write to the source.
    pub read_only: bool,
}

/// Ids this call created. No JIT and no name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Started {
    /// Private `DinD` container id.
    pub dind_id: String,
    /// Runner container id.
    pub runner_id: String,
}

/// Bollard create inputs. Platform is on `options` and on [`CreateProjection`].
///
/// Bollard 0.21.1 has no platform field on [`ContainerCreateBody`].
#[derive(Debug, Clone, PartialEq)]
pub struct BollardCreate {
    /// Query options for `create_container`, including platform.
    pub options: CreateContainerOptions,
    /// Body for `create_container`. No JIT.
    pub config: ContainerCreateBody,
}

/// Project an audited runner plan. Stdin carries JIT. The plan does not.
///
/// # Errors
///
/// Returns [`HostError::PrivilegedRunner`] or [`HostError::ForbiddenMount`]
/// from [`audit_plan`], including JIT in env, cmd, or labels.
pub fn runner_create(plan: &ContainerPlan) -> Result<CreateProjection, HostError> {
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
        resource_budget: None,
    })
}

/// Join `spec` to the private `DinD` network namespace.
///
/// Published service ports and Testcontainers then bind on the runner's localhost.
/// `dind_id` must be one Docker container id. `host` and other modes are rejected.
///
/// # Errors
///
/// Returns [`HostError::ForbiddenMount`] when `dind_id` is not 64 hex digits.
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

/// Private `DinD` create. Privilege is not a flag on the runner plan.
///
/// Mounts are the runner plan's socket volume at `/run`, the work volume at
/// the Actions runner's `/home/runner/_work`, and a DinD-only volume at `/var/lib/docker`. The data
/// volume is not on the runner. The action archive volume is not mounted
/// here. vfs on the container layer slows later Testcontainers starts.
///
/// # Errors
///
/// Returns [`HostError::ForbiddenMount`] when `private_volume` is not one private name.
pub fn dind_create(private_volume: &str) -> Result<CreateProjection, HostError> {
    let runner = runner_plan(private_volume)?;
    let mut mounts = runner.mounts;
    mounts.push(Mount {
        source: format!("volume:{private_volume}-docker"),
        target: "/var/lib/docker".to_owned(),
    });
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
        resource_budget: None,
    })
}

/// Bollard config for [`bollard::Docker::create_container`]. No JIT parameter.
///
/// # Errors
///
/// Returns [`HostError::ForbiddenMount`] when the platform is not `linux/amd64`
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

/// Create volumes, start `DinD`, start the runner, then write `jit` on stdin.
///
/// Empty `jit` returns before any container create. Failure after a create
/// removes only ids this call created.
///
/// # Errors
///
/// Returns [`HostError::EmptyJit`] when `jit` is empty.
/// Returns [`HostError::ForbiddenMount`] for a rejected volume name.
/// Returns [`HostError::Docker`] when Docker rejects a call. The error text
/// does not include `jit`.
pub async fn start_pair(
    docker: &Docker,
    private_volume: &str,
    jit: &[u8],
) -> Result<Started, HostError> {
    let partial = Box::pin(crate::stage::start_pair_until(
        docker,
        private_volume,
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
