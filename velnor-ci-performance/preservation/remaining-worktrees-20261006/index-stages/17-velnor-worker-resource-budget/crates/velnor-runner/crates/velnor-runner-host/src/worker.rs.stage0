//! Docker create projections for one runner and its private `DinD`.
//!
//! JIT is not a projection field. The launch sequence writes it on runner stdin.

use std::collections::HashMap;
use std::path::Path;
use std::time::Duration;

use bollard::Docker;
use bollard::models::{
    ContainerCreateBody, HostConfig, HostConfigCgroupnsModeEnum, Mount as DockerMount,
    MountBindOptions, MountType,
};
use bollard::query_parameters::{
    AttachContainerOptionsBuilder, CreateContainerOptions, StartContainerOptions,
};
use tokio::io::AsyncWriteExt;
use tokio::time::timeout;

use crate::docker_spec::{ContainerPlan, Mount, audit_plan};
use crate::error::HostError;

mod prepared;
mod projection;
mod resources;
pub(crate) use prepared::{PreparedDind, prepare_dind_until, start_runner_until};
pub(super) use projection::{container_labels, container_name, runner_create_for_identity};
pub(super) use projection::{dind_create, identity_labels_match, launch_identity_labels_match};
pub(super) use resources::{
    confirmed_not_found, create_owned_volumes, list_launch, probe_dind, remove_owned_volumes,
    verify_container, verify_engine,
};

#[cfg(test)]
mod projection_tests;

const PLATFORM: &str = "linux/amd64";
pub(super) const DIND_IMAGE: &str = "velnor-dind:29.8.2";
const RUNNER_ENTRYPOINT: &str = "/usr/local/bin/velnor-runner-entrypoint";
const DIND_ENTRYPOINT: [&str; 1] = ["/usr/local/bin/velnor-dind-entrypoint"];
const CONTAINER_CREATE_TIMEOUT: Duration = Duration::from_secs(10);
const CONTAINER_START_TIMEOUT: Duration = Duration::from_secs(10);
const JIT_DELIVERY_TIMEOUT: Duration = Duration::from_secs(10);

/// One Docker create. Not a bollard type. JIT is not a field.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CreateProjection {
    /// Deterministic Docker name for this durable launch.
    pub name: Option<String>,
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
}

/// One controller-owned host bind in a runner create projection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct BindMount {
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
pub(crate) struct BollardCreate {
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
pub(crate) fn runner_create(plan: &ContainerPlan) -> Result<CreateProjection, HostError> {
    audit_plan(plan)?;
    if plan.cmd.iter().any(|item| cmd_names_jit(item)) {
        return Err(HostError::ForbiddenMount);
    }
    Ok(CreateProjection {
        name: None,
        image: plan.image.clone(),
        platform: plan.platform.clone(),
        env: plan.env.clone(),
        cmd: plan.cmd.clone(),
        entrypoint: vec![RUNNER_ENTRYPOINT.to_owned()],
        user: Some("runner".to_owned()),
        working_dir: Some("/home/runner".to_owned()),
        labels: plan.labels.clone(),
        mounts: plan.mounts.clone(),
        bind_mounts: Vec::new(),
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

/// Bollard config for [`bollard::Docker::create_container`]. No JIT parameter.
///
/// # Errors
///
/// Returns [`HostError::ForbiddenMount`] when the platform is not `linux/amd64`
/// or a mount or label cannot be sent.
pub(crate) fn bollard_create(spec: &CreateProjection) -> Result<BollardCreate, HostError> {
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
            name: spec.name.clone(),
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

fn host_config(spec: &CreateProjection) -> Result<HostConfig, HostError> {
    let mut mounts = docker_mounts(&spec.mounts)?.unwrap_or_default();
    mounts.extend(bind_mounts(spec)?);
    Ok(HostConfig {
        cgroupns_mode: Some(HostConfigCgroupnsModeEnum::PRIVATE),
        privileged: Some(spec.privileged),
        mounts: Some(mounts),
        network_mode: spec.network_mode.clone(),
        ..Default::default()
    })
}

fn bind_mounts(spec: &CreateProjection) -> Result<Vec<DockerMount>, HostError> {
    let archive_env = "ACTIONS_RUNNER_ACTION_ARCHIVE_CACHE=/opt/velnor/action-archives";
    let archive_envs = spec
        .env
        .iter()
        .filter(|entry| entry.starts_with("ACTIONS_RUNNER_ACTION_ARCHIVE_CACHE="))
        .count();
    if spec.bind_mounts.len() > 1 || archive_envs != if spec.bind_mounts.is_empty() { 0 } else { 1 }
    {
        return Err(HostError::ForbiddenMount);
    }
    for mount in &spec.bind_mounts {
        if mount.target != "/opt/velnor/action-archives"
            || !Path::new(&mount.source).is_absolute()
            || !mount.read_only
            || !spec.env.iter().any(|entry| entry == archive_env)
        {
            return Err(HostError::ForbiddenMount);
        }
    }
    Ok(spec
        .bind_mounts
        .iter()
        .map(|mount| DockerMount {
            target: Some(mount.target.clone()),
            source: Some(mount.source.clone()),
            typ: Some(MountType::BIND),
            read_only: Some(mount.read_only),
            bind_options: Some(MountBindOptions {
                create_mountpoint: Some(false),
                ..Default::default()
            }),
            ..Default::default()
        })
        .collect())
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

fn volume_name(source: &str) -> Result<&str, HostError> {
    source
        .strip_prefix("volume:")
        .filter(|name| !name.is_empty())
        .ok_or(HostError::ForbiddenMount)
}

fn container_id(id: &str) -> bool {
    (12..=64).contains(&id.len()) && id.bytes().all(|byte| byte.is_ascii_hexdigit())
}

pub(crate) async fn create_only(
    docker: &Docker,
    spec: &CreateProjection,
) -> Result<String, HostError> {
    resources::refuse_existing(docker, spec).await?;
    let created = bollard_create(spec)?;
    let response = timeout(
        CONTAINER_CREATE_TIMEOUT,
        docker.create_container(Some(created.options), created.config),
    )
    .await
    .map_err(|_| HostError::ContainerCreateUncertain)?
    .map_err(|_| HostError::ContainerCreateUncertain)?;
    if !container_id(&response.id) {
        Err(HostError::ContainerCreateUncertain)
    } else {
        Ok(response.id)
    }
}

pub(crate) async fn start_id(docker: &Docker, id: &str) -> Result<(), HostError> {
    timeout(
        CONTAINER_START_TIMEOUT,
        docker.start_container(id, None::<StartContainerOptions>),
    )
    .await
    .map_err(|_| HostError::ContainerStartUncertain)?
    .map_err(|_| HostError::ContainerStartUncertain)
}

pub(crate) async fn deliver_jit(docker: &Docker, id: &str, jit: &[u8]) -> Result<(), HostError> {
    let options = AttachContainerOptionsBuilder::new()
        .stdin(true)
        .stream(true)
        .build();
    timeout(JIT_DELIVERY_TIMEOUT, async {
        let mut attached = docker
            .attach_container(id, Some(options))
            .await
            .map_err(|_| HostError::JitDeliveryUncertain)?;
        attached
            .input
            .write_all(jit)
            .await
            .map_err(|_| HostError::JitDeliveryUncertain)?;
        attached
            .input
            .shutdown()
            .await
            .map_err(|_| HostError::JitDeliveryUncertain)?;
        Ok(())
    })
    .await
    .map_err(|_| HostError::JitDeliveryUncertain)?
}
