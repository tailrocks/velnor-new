//! Match one publisher against its exact Docker inspect record.

use bollard::models::{ContainerInspectResponse, ContainerStateStatusEnum};

use crate::{
    launch::resource_probe::publication::projection::PublicationProjection,
    worker::execution_authorization::VerifiedMetadataVolume,
};

pub(super) fn matches_created(
    projection: &PublicationProjection,
    volume: &VerifiedMetadataVolume,
    response: &ContainerInspectResponse,
    id: &str,
) -> bool {
    matches_identity(projection, volume, response, id)
        && response.state.as_ref().is_some_and(|state| {
            state.status == Some(ContainerStateStatusEnum::CREATED) && state.running == Some(false)
        })
}

pub(super) fn matches_exited(
    projection: &PublicationProjection,
    volume: &VerifiedMetadataVolume,
    response: &ContainerInspectResponse,
    id: &str,
) -> bool {
    matches_identity(projection, volume, response, id)
        && response.state.as_ref().is_some_and(|state| {
            state.status == Some(ContainerStateStatusEnum::EXITED)
                && state.running == Some(false)
                && state.exit_code == Some(0)
                && state.oom_killed == Some(false)
        })
}

fn matches_identity(
    projection: &PublicationProjection,
    volume: &VerifiedMetadataVolume,
    response: &ContainerInspectResponse,
    id: &str,
) -> bool {
    matches_identity_with_volume(projection, volume.name(), volume.mountpoint(), response, id)
}

pub(in crate::launch::resource_probe::publication) fn matches_recovery_identity(
    projection: &PublicationProjection,
    volume_name: &str,
    mountpoint: &str,
    response: &ContainerInspectResponse,
    id: &str,
) -> bool {
    matches_identity_with_volume(projection, volume_name, mountpoint, response, id)
}

fn matches_identity_with_volume(
    projection: &PublicationProjection,
    volume_name: &str,
    mountpoint: &str,
    response: &ContainerInspectResponse,
    id: &str,
) -> bool {
    let Some(config) = response.config.as_ref() else {
        return false;
    };
    let Some(host) = response.host_config.as_ref() else {
        return false;
    };
    response.id.as_deref() == Some(id)
        && response.name.as_deref() == Some(format!("/{}", projection.name).as_str())
        && response.image.as_deref() == Some(projection.request_image())
        && response.path.as_deref() == Some("/velnor/resource-probe")
        && response
            .args
            .as_deref()
            .is_some_and(|args| args == ["--mode".to_owned(), projection.publisher_mode.clone()])
        && container_config_matches(projection, config, id)
        && host.privileged == Some(false)
        && host.readonly_rootfs == Some(true)
        && host.network_mode.as_deref() == Some("none")
        && host.cap_drop.as_deref() == Some(["ALL".to_owned()].as_slice())
        && host.cap_add.as_deref() == Some(["DAC_OVERRIDE".to_owned()].as_slice())
        && publication_security_matches(host)
        && host.cgroupns_mode == Some(bollard::models::HostConfigCgroupnsModeEnum::PRIVATE)
        && host.cpu_period == Some(100_000)
        && host.cpu_quota == Some(10_000)
        && host.memory == Some(128 * 1024 * 1024)
        && host.memory_swap == Some(128 * 1024 * 1024)
        && host.pids_limit == Some(4)
        && host.mounts.as_ref().is_some_and(|mounts| {
            mounts.len() == 1
                && mounts[0].target.as_deref() == Some("/opt/velnor")
                && mounts[0].source.as_deref() == Some(projection.metadata_volume.as_str())
                && mounts[0].read_only == Some(false)
                && mounts[0]
                    .volume_options
                    .as_ref()
                    .is_some_and(|options| options.no_copy == Some(true))
        })
        && response
            .mounts
            .as_ref()
            .is_some_and(|mounts| exact_metadata_mount(mounts, volume_name, mountpoint))
        && host.pid_mode.as_deref().is_none_or(str::is_empty)
        && host.binds.as_ref().is_none_or(Vec::is_empty)
        && host.devices.as_ref().is_none_or(Vec::is_empty)
        && host.device_requests.as_ref().is_none_or(Vec::is_empty)
        && host
            .port_bindings
            .as_ref()
            .is_none_or(std::collections::HashMap::is_empty)
        && host.restart_policy.as_ref().is_none_or(|policy| {
            policy.name.is_none_or(|name| {
                matches!(
                    name,
                    bollard::models::RestartPolicyNameEnum::EMPTY
                        | bollard::models::RestartPolicyNameEnum::NO
                )
            })
        })
}

fn container_config_matches(
    projection: &PublicationProjection,
    actual: &bollard::models::ContainerConfig,
    id: &str,
) -> bool {
    let Some(short_id) = id.get(..12) else {
        return false;
    };
    let mut actual = actual.clone();
    if actual
        .hostname
        .as_deref()
        .is_some_and(|value| !value.is_empty() && value != short_id)
        || actual
            .domainname
            .as_deref()
            .is_some_and(|value| !value.is_empty())
        || actual.network_disabled == Some(false)
    {
        return false;
    }
    actual.hostname = None;
    actual.domainname = None;
    if actual.cmd.as_ref().is_some_and(Vec::is_empty) {
        actual.cmd = None;
    }
    if actual.network_disabled.is_none() {
        actual.network_disabled = Some(true);
    }
    if actual.exposed_ports.as_ref().is_some_and(Vec::is_empty) {
        actual.exposed_ports = None;
    }
    if actual.volumes.as_ref().is_some_and(Vec::is_empty) {
        actual.volumes = None;
    }
    if actual.on_build.as_ref().is_some_and(Vec::is_empty) {
        actual.on_build = None;
    }
    if actual.args_escaped == Some(false) {
        actual.args_escaped = None;
    }
    actual == projection.expected_config
}

fn publication_security_matches(host: &bollard::models::HostConfig) -> bool {
    host.auto_remove == Some(false)
        && host.security_opt.as_deref() == Some(["no-new-privileges:true".to_owned()].as_slice())
}

pub(super) fn exact_metadata_mount(
    mounts: &[bollard::models::MountPoint],
    volume_name: &str,
    mountpoint: &str,
) -> bool {
    mounts.len() == 1
        && mounts[0].typ.as_deref() == Some("volume")
        && mounts[0].name.as_deref() == Some(volume_name)
        && mounts[0].destination.as_deref() == Some("/opt/velnor")
        && mounts[0].source.as_deref() == Some(mountpoint)
        && mounts[0].driver.as_deref() == Some("local")
        && mounts[0].rw == Some(true)
}

#[cfg(test)]
mod tests {
    use bollard::models::{HostConfig, MountPoint};

    use super::{exact_metadata_mount, publication_security_matches};

    fn mount() -> MountPoint {
        MountPoint {
            typ: Some("volume".to_owned()),
            name: Some("owned-volume".to_owned()),
            source: Some("/var/lib/docker/volumes/owned-volume/_data".to_owned()),
            destination: Some("/opt/velnor".to_owned()),
            driver: Some("local".to_owned()),
            rw: Some(true),
            ..Default::default()
        }
    }

    #[test]
    fn inspect_must_show_the_exact_owned_metadata_volume() {
        let good = mount();
        assert!(exact_metadata_mount(
            std::slice::from_ref(&good),
            "owned-volume",
            "/var/lib/docker/volumes/owned-volume/_data"
        ));
        let mutations: [fn(&mut MountPoint); 4] = [
            |value: &mut MountPoint| value.name = Some("foreign-volume".to_owned()),
            |value: &mut MountPoint| value.destination = Some("/tmp/metadata".to_owned()),
            |value: &mut MountPoint| value.rw = Some(false),
            |value: &mut MountPoint| value.typ = Some("bind".to_owned()),
        ];
        for mutate in mutations {
            let mut changed = mount();
            mutate(&mut changed);
            assert!(!exact_metadata_mount(
                std::slice::from_ref(&changed),
                "owned-volume",
                "/var/lib/docker/volumes/owned-volume/_data"
            ));
        }
        assert!(!exact_metadata_mount(
            &[mount(), mount()],
            "owned-volume",
            "/var/lib/docker/volumes/owned-volume/_data"
        ));
    }

    #[test]
    fn inspect_must_show_no_new_privileges_and_no_auto_removal() {
        let good = HostConfig {
            auto_remove: Some(false),
            security_opt: Some(vec!["no-new-privileges:true".to_owned()]),
            ..Default::default()
        };
        assert!(publication_security_matches(&good));

        let mutations: [fn(&mut HostConfig); 3] = [
            |value| value.auto_remove = Some(true),
            |value| value.auto_remove = None,
            |value| value.security_opt = Some(vec!["no-new-privileges:false".to_owned()]),
        ];
        for mutate in mutations {
            let mut changed = good.clone();
            mutate(&mut changed);
            assert!(!publication_security_matches(&changed));
        }
    }
}
