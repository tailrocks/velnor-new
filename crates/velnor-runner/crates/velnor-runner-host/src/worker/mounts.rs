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
use super::projection_types::BindMountKind;
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
    let archive_envs = spec
        .env
        .iter()
        .filter(|entry| entry.starts_with("ACTIONS_RUNNER_ACTION_ARCHIVE_CACHE="))
        .count();
    let exact_archive_env = spec
        .env
        .iter()
        .filter(|entry| {
            entry.as_str() == "ACTIONS_RUNNER_ACTION_ARCHIVE_CACHE=/opt/velnor/action-archives"
        })
        .count();
    let archive_mounts = spec
        .bind_mounts
        .iter()
        .filter(|mount| mount.kind == BindMountKind::ActionArchive)
        .count();
    let guest_image_mounts = spec
        .bind_mounts
        .iter()
        .filter(|mount| mount.kind == BindMountKind::GuestImage)
        .count();
    if spec.bind_mounts.len() > 2
        || archive_mounts > 1
        || guest_image_mounts > 1
        || archive_envs != archive_mounts
        || exact_archive_env != archive_envs
        || spec.env.len() != archive_envs
    {
        return Err(HostError::ForbiddenMount);
    }
    for mount in &spec.bind_mounts {
        let valid_target = match mount.kind {
            BindMountKind::ActionArchive => mount.target == "/opt/velnor/action-archives",
            BindMountKind::GuestImage => mount.target == "/opt/velnor/guest-image.json",
        };
        if !valid_target || !Path::new(&mount.source).is_absolute() || !mount.read_only {
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

#[cfg(test)]
mod tests {
    use bollard::models::MountType;

    use crate::docker_spec::runner_plan;
    use crate::worker::{BindMount, runner_create, test_resource_budget};

    use super::*;

    fn runner() -> Result<CreateProjection, HostError> {
        runner_create(&runner_plan("worker_a")?, test_resource_budget()?)
    }

    #[test]
    fn guest_image_bind_is_exact_read_only_and_has_no_environment_channel() -> Result<(), HostError>
    {
        let mut spec = runner()?;
        spec.bind_mounts.push(BindMount::guest_image(
            "/var/lib/velnor/guest-image/essential/abc.json".to_owned(),
        ));

        let host = host_config(&spec)?;
        let mounts = host.mounts.as_ref().ok_or(HostError::Docker)?;
        let guest_image = mounts
            .iter()
            .find(|mount| mount.target.as_deref() == Some("/opt/velnor/guest-image.json"))
            .ok_or(HostError::Docker)?;
        assert_eq!(
            guest_image.source.as_deref(),
            Some("/var/lib/velnor/guest-image/essential/abc.json")
        );
        assert_eq!(guest_image.typ, Some(MountType::BIND));
        assert_eq!(guest_image.read_only, Some(true));
        assert_eq!(
            guest_image
                .bind_options
                .as_ref()
                .and_then(|options| options.create_mountpoint),
            Some(false)
        );
        assert!(spec.env.is_empty());
        Ok(())
    }

    #[test]
    fn guest_image_bind_rejects_writable_mounts_and_other_targets() -> Result<(), HostError> {
        let mut spec = runner()?;
        let mut mount = BindMount::guest_image("/var/lib/velnor/image.json".to_owned());
        mount.read_only = false;
        spec.bind_mounts.push(mount);
        assert_eq!(host_config(&spec), Err(HostError::ForbiddenMount));

        let mut spec = runner()?;
        let mut mount = BindMount::guest_image("/var/lib/velnor/image.json".to_owned());
        mount.target = "/home/runner/.ssh".to_owned();
        spec.bind_mounts.push(mount);
        assert_eq!(host_config(&spec), Err(HostError::ForbiddenMount));
        Ok(())
    }
}
