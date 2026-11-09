// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use bollard::models::{ContainerInspectResponse, MountType};

use crate::error::QualificationError as HostError;
use crate::resources::TrackedContainer;
use crate::{QUALIFIER_LABEL, valid_container_id};

pub(super) fn validate_host_config(
    inspect: &ContainerInspectResponse,
    role: &str,
    volumes: &[&str],
) -> Result<(), HostError> {
    let host = inspect.host_config.as_ref().ok_or(HostError::Ownership)?;
    if host.privileged != Some(role == "dind")
        || host.network_mode.as_deref() != Some("none")
        || host
            .port_bindings
            .as_ref()
            .is_some_and(|ports| !ports.is_empty())
    {
        return Err(HostError::Ownership);
    }
    if role == "resource-probe" {
        return validate_probe_host_config(inspect, host, volumes);
    }
    let expected_targets = match (role, volumes.len()) {
        ("dind", 2) => &["/run", "/var/lib/docker"][..],
        ("runner-socket-query", 1) => &["/run"][..],
        (_, 0) => &[][..],
        _ => return Err(HostError::Ownership),
    };
    let mounts = host.mounts.as_deref().unwrap_or_default();
    if mounts.len() != volumes.len() {
        return Err(HostError::Ownership);
    }
    for ((mount, volume), target) in mounts.iter().zip(volumes).zip(expected_targets) {
        if mount.typ != Some(MountType::VOLUME)
            || mount.source.as_deref() != Some(*volume)
            || mount.target.as_deref() != Some(*target)
            || mount.read_only == Some(true)
        {
            return Err(HostError::Ownership);
        }
    }
    Ok(())
}

fn validate_probe_host_config(
    inspect: &ContainerInspectResponse,
    host: &bollard::models::HostConfig,
    roots: &[&str],
) -> Result<(), HostError> {
    let config = inspect.config.as_ref().ok_or(HostError::Ownership)?;
    let mounts = host.mounts.as_deref().unwrap_or_default();
    if roots.len() != 1
        || config.user.as_deref() != Some("65532:65532")
        || config.working_dir.as_deref() != Some("/")
        || config.cmd.as_ref().is_some_and(|args| !args.is_empty())
        || config.env.as_deref()
            != Some(
                &["PATH=/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin".to_owned()][..],
            )
        || config.entrypoint.as_deref() != Some(&["/velnor/resource-probe".to_owned()][..])
        || host.binds.as_ref().is_some_and(|v| !v.is_empty())
        || host.volumes_from.as_ref().is_some_and(|v| !v.is_empty())
        || host.devices.as_ref().is_some_and(|v| !v.is_empty())
        || host
            .device_cgroup_rules
            .as_ref()
            .is_some_and(|v| !v.is_empty())
        || host.device_requests.as_ref().is_some_and(|v| !v.is_empty())
        || host.cap_add.as_ref().is_some_and(|v| !v.is_empty())
        || host.tmpfs.as_ref().is_some_and(|v| !v.is_empty())
        || host.group_add.as_ref().is_some_and(|v| !v.is_empty())
        || host.links.as_ref().is_some_and(|v| !v.is_empty())
        || host.extra_hosts.as_ref().is_some_and(|v| !v.is_empty())
        || host.sysctls.as_ref().is_some_and(|v| !v.is_empty())
        || host.cgroup_parent.as_ref().is_some_and(|v| !v.is_empty())
        || host.uts_mode.as_ref().is_some_and(|v| !v.is_empty())
        || host.userns_mode.as_ref().is_some_and(|v| !v.is_empty())
        || host
            .ipc_mode
            .as_deref()
            .is_some_and(|v| !matches!(v, "" | "private"))
        || host.readonly_rootfs != Some(true)
        || host.pid_mode.as_deref() != Some("host")
        || host.cgroupns_mode != Some(bollard::models::HostConfigCgroupnsModeEnum::HOST)
        || host.cap_drop.as_deref() != Some(&["ALL".to_owned()][..])
        || host.security_opt.as_deref() != Some(&["no-new-privileges:true".to_owned()][..])
        || host.auto_remove != Some(false)
        || mounts.len() != 1
        || mounts[0].typ != Some(MountType::BIND)
        || mounts[0].source.as_deref() != roots.first().copied()
        || mounts[0].target.as_deref() != Some("/velnor/docker-root")
        || mounts[0].read_only != Some(true)
    {
        return Err(HostError::Ownership);
    }
    Ok(())
}

pub(super) fn validate_ownership<'a>(
    run_id: &str,
    inspect: &'a bollard::models::ContainerInspectResponse,
    tracked: &TrackedContainer,
) -> Result<&'a str, HostError> {
    let id = inspect.id.as_deref().ok_or(HostError::Ownership)?;
    valid_container_id(id)?;
    if tracked.id.as_deref().is_some_and(|recorded| recorded != id)
        || inspect
            .name
            .as_deref()
            .is_none_or(|name| name.strip_prefix('/').unwrap_or(name) != tracked.name)
    {
        return Err(HostError::Ownership);
    }
    let labels = inspect
        .config
        .as_ref()
        .and_then(|config| config.labels.as_ref())
        .ok_or(HostError::Ownership)?;
    if labels.get(QUALIFIER_LABEL).map(String::as_str) != Some(run_id)
        || labels
            .get("io.velnor.runner-image-qualifier.role")
            .map(String::as_str)
            != Some(tracked.role.as_str())
    {
        return Err(HostError::Ownership);
    }
    Ok(id)
}
