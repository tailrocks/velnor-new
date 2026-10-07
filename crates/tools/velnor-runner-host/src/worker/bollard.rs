//! Convert an audited worker projection into Bollard's Docker request types.

use std::collections::HashMap;

use ::bollard::models::{
    ContainerCreateBody, HostConfig, Mount as DockerMount, MountImageOptions, MountType,
};
use ::bollard::query_parameters::CreateContainerOptions;

use crate::HostError;
use crate::docker_spec::{ContainerPlan, ImageMount, Mount, audit_plan, resolve_runner_profile};

use super::{CreateProjection, PLATFORM};

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
    audit_projection(spec)?;
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
            name: Some(spec.name.clone()),
            platform: PLATFORM.to_owned(),
        },
        config,
    })
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
        group_add: none_if_empty(&spec.group_add),
        mounts: docker_mounts(&spec.mounts, &spec.image_mounts)?,
        network_mode: spec.network_mode.clone(),
        readonly_rootfs: spec.readonly_rootfs.then_some(true),
        security_opt: none_if_empty(&spec.security_opts),
        ..Default::default()
    })
}

fn docker_mounts(
    mounts: &[Mount],
    image_mounts: &[ImageMount],
) -> Result<Option<Vec<DockerMount>>, HostError> {
    if mounts.is_empty() && image_mounts.is_empty() {
        return Ok(None);
    }
    let mut out = Vec::with_capacity(mounts.len() + image_mounts.len());
    for mount in mounts {
        out.push(docker_mount(mount)?);
    }
    for mount in image_mounts {
        out.push(docker_image_mount(mount)?);
    }
    Ok(Some(out))
}

fn docker_mount(mount: &Mount) -> Result<DockerMount, HostError> {
    let (typ, source) = mount_source(&mount.source)?;
    Ok(DockerMount {
        target: Some(mount.target.clone()),
        source: Some(source),
        typ: Some(typ),
        read_only: mount.read_only.then_some(true),
        ..Default::default()
    })
}

fn docker_image_mount(mount: &ImageMount) -> Result<DockerMount, HostError> {
    if !mount.subpath.starts_with("home/runner/")
        || mount
            .subpath
            .split('/')
            .any(|part| part.is_empty() || part == ".." || part == ".")
        || mount.target != format!("/{}", mount.subpath)
    {
        return Err(HostError::ForbiddenMount);
    }
    Ok(DockerMount {
        target: Some(mount.target.clone()),
        source: Some(mount.image.clone()),
        typ: Some(MountType::IMAGE),
        read_only: Some(true),
        image_options: Some(MountImageOptions {
            subpath: Some(mount.subpath.clone()),
        }),
        ..Default::default()
    })
}

fn audit_projection(spec: &CreateProjection) -> Result<(), HostError> {
    if spec.privileged {
        let worker = spec
            .name
            .strip_suffix("-dind")
            .filter(|name| !name.is_empty())
            .ok_or(HostError::ForbiddenMount)?;
        if crate::worker::dind_create(worker)? == *spec {
            return Ok(());
        }
        let profile = resolve_runner_profile("ubuntu-24.04-amd64", "ubuntu-24.04-scale-set")?;
        return (crate::worker::dind_create_for_profile(worker, &profile)? == *spec)
            .then_some(())
            .ok_or(HostError::ForbiddenMount);
    }
    let plan = ContainerPlan {
        name: spec.name.clone(),
        privileged: spec.privileged,
        platform: spec.platform.clone(),
        readonly_rootfs: spec.readonly_rootfs,
        image: spec.image.clone(),
        env: spec.env.clone(),
        cmd: spec.cmd.clone(),
        labels: spec.labels.clone(),
        mounts: spec.mounts.clone(),
        image_mounts: spec.image_mounts.clone(),
        group_add: spec.group_add.clone(),
        security_opts: spec.security_opts.clone(),
    };
    audit_plan(&plan)?;
    if spec
        .network_mode
        .as_deref()
        .is_some_and(|mode| !valid_container_network_mode(mode))
    {
        return Err(HostError::ForbiddenMount);
    }
    if spec.image_mounts.is_empty() {
        return Ok(());
    }
    spec.network_mode
        .as_deref()
        .is_some_and(valid_container_network_mode)
        .then_some(())
        .ok_or(HostError::ForbiddenMount)
}

fn valid_container_network_mode(mode: &str) -> bool {
    mode.strip_prefix("container:").is_some_and(|id| {
        (12..=64).contains(&id.len()) && id.bytes().all(|byte| byte.is_ascii_hexdigit())
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
