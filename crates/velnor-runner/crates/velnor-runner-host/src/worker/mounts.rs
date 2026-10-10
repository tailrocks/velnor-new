//! Docker host-config projection for a [`CreateProjection`](super::CreateProjection).
//!
//! Split from `worker.rs` by the 400-line repo-size gate. Volume mounts,
//! the single controller-owned action-archive bind, and label encoding live
//! here; no JIT flows through these helpers.

use std::collections::HashMap;
use std::path::Path;

use bollard::models::{
    HostConfig, HostConfigCgroupnsModeEnum, Mount as DockerMount, MountBindOptions, MountType,
};

use super::CreateProjection;
use crate::docker_spec::Mount;
use crate::error::HostError;

pub(super) fn host_config(spec: &CreateProjection) -> Result<HostConfig, HostError> {
    let budget = spec.resource_budget;
    let mut mounts = docker_mounts(&spec.mounts)?.unwrap_or_default();
    mounts.extend(bind_mounts(spec)?);
    let limits = if spec.privileged {
        budget.dind()
    } else {
        budget.runner()
    };
    Ok(HostConfig {
        cgroupns_mode: Some(HostConfigCgroupnsModeEnum::PRIVATE),
        memory: Some(limits.memory_bytes),
        memory_swap: Some(limits.memory_bytes),
        nano_cpus: Some(limits.nano_cpus),
        privileged: Some(spec.privileged),
        mounts: if mounts.is_empty() {
            None
        } else {
            Some(mounts)
        },
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
    if spec.bind_mounts.len() > 1 || archive_envs != usize::from(!spec.bind_mounts.is_empty()) {
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
        read_only: seed_read_only(&mount.source),
        ..Default::default()
    })
}

fn seed_read_only(source: &str) -> Option<bool> {
    (source == crate::docker_spec::SEED_VOLUME
        || source == crate::docker_spec::ACTION_ARCHIVE_VOLUME)
        .then_some(true)
}

pub(super) fn mount_source(source: &str) -> Result<(MountType, String), HostError> {
    Ok((MountType::VOLUME, volume_name(source)?.to_owned()))
}

pub(super) fn label_map(labels: &[String]) -> Result<Option<HashMap<String, String>>, HostError> {
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
