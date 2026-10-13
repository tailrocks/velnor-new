//! Docker create projection for one runner and its private `DinD`.
//!
//! JIT is not a field. `start_pair` writes it on stdin and does not store it.

use std::collections::HashMap;

use bollard::Docker;
use bollard::models::{
    ContainerCreateBody, HostConfig, Mount as DockerMount, MountType, VolumeCreateRequest,
};
use bollard::query_parameters::{
    AttachContainerOptionsBuilder, CreateContainerOptions, StartContainerOptions,
};
use tokio::io::AsyncWriteExt;

use crate::docker_spec::{ContainerPlan, Mount, audit_plan, runner_plan};
use crate::error::HostError;
use crate::stage::PairStop;

const PLATFORM: &str = "linux/amd64";
const DIND_IMAGE: &str = "velnor-dind:29.8.2";

/// One Docker create. Not a bollard type. JIT is not a field.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreateProjection {
    /// Image reference.
    pub image: String,
    /// OCI platform. Always `linux/amd64`.
    pub platform: String,
    /// Env pairs. Empty when the image environment stands.
    pub env: Vec<String>,
    /// Command. Empty when the image entrypoint stands.
    pub cmd: Vec<String>,
    /// `key=value` labels. No JIT.
    pub labels: Vec<String>,
    /// Mounts. Volume sources use `volume:<name>`.
    pub mounts: Vec<Mount>,
    /// Host privilege. False for the runner. True only for private `DinD`.
    pub privileged: bool,
    /// `OpenStdin`. True only for the runner channel.
    pub open_stdin: bool,
    /// `container:<id>` joins that container's network namespace. Runner only.
    pub network_mode: Option<String>,
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
        image: plan.image.clone(),
        platform: plan.platform.clone(),
        env: plan.env.clone(),
        cmd: plan.cmd.clone(),
        labels: plan.labels.clone(),
        mounts: plan.mounts.clone(),
        privileged: false,
        open_stdin: true,
        network_mode: None,
    })
}

/// Join `spec` to the private `DinD` network namespace.
///
/// Published service ports and Testcontainers then bind on the runner's localhost.
/// `dind_id` must be one Docker container id. `host` and other modes are rejected.
///
/// # Errors
///
/// Returns [`HostError::ForbiddenMount`] when `dind_id` is not a hex container id.
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
    (12..=64).contains(&id.len()) && id.bytes().all(|byte| byte.is_ascii_hexdigit())
}

/// Private `DinD` create. Privilege is not a flag on the runner plan.
///
/// Mounts are the runner plan's socket volume at `/run` and the work volume at
/// `/home/runner/_work`. Same names [`runner_plan`] rejects.
///
/// # Errors
///
/// Returns [`HostError::ForbiddenMount`] when `private_volume` is not one private name.
pub fn dind_create(private_volume: &str) -> Result<CreateProjection, HostError> {
    let runner = runner_plan(private_volume)?;
    Ok(CreateProjection {
        image: DIND_IMAGE.to_owned(),
        platform: runner.platform,
        env: Vec::new(),
        cmd: Vec::new(),
        labels: Vec::new(),
        mounts: runner.mounts,
        privileged: true,
        open_stdin: false,
        network_mode: None,
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
        labels: label_map(&spec.labels)?,
        open_stdin: Some(spec.open_stdin),
        attach_stdin: Some(spec.open_stdin),
        stdin_once: Some(spec.open_stdin),
        host_config: Some(host_config(spec)?),
        ..Default::default()
    };
    Ok(BollardCreate {
        options: CreateContainerOptions {
            name: None,
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
    let partial =
        crate::stage::start_pair_until(docker, private_volume, jit, PairStop::Jit).await?;
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

fn host_config(spec: &CreateProjection) -> Result<HostConfig, HostError> {
    Ok(HostConfig {
        privileged: Some(spec.privileged),
        mounts: docker_mounts(&spec.mounts)?,
        network_mode: spec.network_mode.clone(),
        ..Default::default()
    })
}

fn docker_mounts(mounts: &[Mount]) -> Result<Option<Vec<DockerMount>>, HostError> {
    if mounts.is_empty() {
        return Ok(None);
    }
    let mut out = Vec::with_capacity(mounts.len());
    for mount in mounts {
        out.push(docker_mount(mount)?);
    }
    Ok(Some(out))
}

fn docker_mount(mount: &Mount) -> Result<DockerMount, HostError> {
    let (typ, source) = mount_source(&mount.source)?;
    Ok(DockerMount {
        target: Some(mount.target.clone()),
        source: Some(source),
        typ: Some(typ),
        ..Default::default()
    })
}

fn mount_source(source: &str) -> Result<(MountType, String), HostError> {
    Ok((MountType::VOLUME, volume_name(source)?.to_owned()))
}

fn label_map(labels: &[String]) -> Result<Option<HashMap<String, String>>, HostError> {
    if labels.is_empty() {
        return Ok(None);
    }
    let mut map = HashMap::with_capacity(labels.len());
    for label in labels {
        let (key, value) = label.split_once('=').ok_or(HostError::ForbiddenMount)?;
        if key.is_empty() {
            return Err(HostError::ForbiddenMount);
        }
        map.insert(key.to_owned(), value.to_owned());
    }
    Ok(Some(map))
}

pub(crate) async fn create_named_volumes(
    docker: &Docker,
    plan: &ContainerPlan,
) -> Result<(), HostError> {
    for mount in &plan.mounts {
        let name = volume_name(&mount.source)?;
        let request = VolumeCreateRequest {
            name: Some(name.to_owned()),
            ..Default::default()
        };
        docker
            .create_volume(request)
            .await
            .map_err(|_| HostError::Docker)?;
    }
    Ok(())
}

fn volume_name(source: &str) -> Result<&str, HostError> {
    source
        .strip_prefix("volume:")
        .filter(|name| !name.is_empty())
        .ok_or(HostError::ForbiddenMount)
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
