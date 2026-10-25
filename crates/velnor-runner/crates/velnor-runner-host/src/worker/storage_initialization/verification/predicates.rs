use bollard::models::{ContainerInspectResponse, MountType};

use crate::journal::StorageInitializationClaim;

pub(super) const IMAGE_PATH_ENV: &str =
    "PATH=/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin";

pub(super) struct InitializerIdentity<'a> {
    pub(super) operation_key: &'a str,
    pub(super) helper_name: &'a str,
    pub(super) image_runtime_id: &'a str,
    pub(super) home_volume: &'a str,
    pub(super) work_volume: &'a str,
    pub(super) layout_version: i64,
    pub(super) metadata_volume: Option<&'a str>,
    pub(super) nano_cpus: i64,
    pub(super) memory_bytes: i64,
    pub(super) pids_limit: i64,
}

impl<'a> InitializerIdentity<'a> {
    pub(super) fn from_claim(claim: &'a StorageInitializationClaim) -> Self {
        Self {
            operation_key: claim.operation_key(),
            helper_name: claim.helper_name(),
            image_runtime_id: claim.image_runtime_id(),
            home_volume: claim.home_volume(),
            work_volume: claim.work_volume(),
            layout_version: claim.layout_version(),
            metadata_volume: claim.metadata_volume_name(),
            nano_cpus: claim.nano_cpus(),
            memory_bytes: claim.memory_bytes(),
            pids_limit: claim.pids_limit(),
        }
    }

    pub(super) fn from_pre_jit(identity: &'a crate::journal::PreJitInitializerIdentity) -> Self {
        Self {
            operation_key: identity.operation_key(),
            helper_name: identity.helper_name(),
            image_runtime_id: identity.image_runtime_id(),
            home_volume: identity.home_volume(),
            work_volume: identity.work_volume(),
            layout_version: identity.layout_version(),
            metadata_volume: Some(identity.metadata_volume()),
            nano_cpus: identity.cpu_nano(),
            memory_bytes: identity.memory_bytes(),
            pids_limit: identity.pids_limit(),
        }
    }
}

pub(super) fn container_matches_identity(
    inspected: &ContainerInspectResponse,
    identity: &InitializerIdentity<'_>,
    id: &str,
) -> bool {
    let labels = inspected
        .config
        .as_ref()
        .and_then(|config| config.labels.as_ref());
    inspected.id.as_deref() == Some(id)
        && inspected.name.as_deref() == Some(format!("/{}", identity.helper_name).as_str())
        && network_isolated(inspected)
        && labels.is_some_and(|labels| {
            labels.get("velnor.role").map(String::as_str) == Some("storage-initializer")
                && labels.get("velnor.operation").map(String::as_str)
                    == Some(identity.operation_key)
        })
}

pub(super) fn initializer_config_matches(
    inspected: &ContainerInspectResponse,
    identity: &InitializerIdentity<'_>,
    expected_env: &[String],
) -> bool {
    inspected.config.as_ref().is_some_and(|config| {
        config.image.as_deref() == Some(identity.image_runtime_id)
            && config.user.as_deref() == Some("0:0")
            && config.entrypoint.as_deref() == Some(["/velnor/storage-init".to_owned()].as_slice())
            && config.attach_stdin == Some(true)
            && config.attach_stdout == Some(true)
            && config.attach_stderr == Some(true)
            && config.open_stdin == Some(true)
            && config.stdin_once == Some(true)
            && config.tty == Some(false)
            && config.env.as_deref() == Some(expected_env)
            && config.cmd.as_ref().is_none_or(Vec::is_empty)
    })
}

pub(super) fn initializer_host_config_matches(
    inspected: &ContainerInspectResponse,
    identity: &InitializerIdentity<'_>,
) -> bool {
    let Some(expected_mounts) = expected_initializer_mounts(identity) else {
        return false;
    };
    inspected.host_config.as_ref().is_some_and(|host| {
        host.nano_cpus == Some(identity.nano_cpus)
            && host.memory == Some(identity.memory_bytes)
            && host.memory_swap == Some(identity.memory_bytes)
            && host.pids_limit == Some(identity.pids_limit)
            && host.network_mode.as_deref() == Some("none")
            && host.privileged == Some(false)
            && host.readonly_rootfs == Some(true)
            && host.binds.as_ref().is_none_or(Vec::is_empty)
            && host.cap_drop.as_deref() == Some(["ALL".to_owned()].as_slice())
            && host
                .cap_add
                .as_deref()
                .is_some_and(|items| capabilities_match(items, &["CHOWN", "FOWNER"]))
            && host.security_opt.as_deref()
                == Some(["no-new-privileges:true".to_owned()].as_slice())
            && host.mounts.as_ref().is_some_and(|mounts| {
                mounts.len() == expected_mounts.len()
                    && expected_mounts.iter().all(|(source, target)| {
                        mounts.iter().any(|mount| {
                            mount.source.as_deref() == Some(*source)
                                && mount.target.as_deref() == Some(*target)
                                && mount.typ == Some(MountType::VOLUME)
                                && !mount.read_only.unwrap_or(false)
                                && mount
                                    .volume_options
                                    .as_ref()
                                    .is_some_and(|options| options.no_copy == Some(true))
                        })
                    })
            })
    })
}

pub(super) fn initializer_mounts_match(
    inspected: &ContainerInspectResponse,
    identity: &InitializerIdentity<'_>,
) -> bool {
    let Some(expected_mounts) = expected_initializer_mounts(identity) else {
        return false;
    };
    inspected.mounts.as_ref().is_some_and(|mounts| {
        mounts.len() == expected_mounts.len()
            && expected_mounts.iter().all(|(name, destination)| {
                mounts.iter().any(|mount| {
                    mount.name.as_deref() == Some(*name)
                        && mount.destination.as_deref() == Some(*destination)
                        && mount.rw == Some(true)
                })
            })
    })
}

fn expected_initializer_mounts<'a>(
    identity: &InitializerIdentity<'a>,
) -> Option<Vec<(&'a str, &'static str)>> {
    let mut expected = vec![
        (identity.home_volume, "/velnor/init/home"),
        (identity.work_volume, "/velnor/init/work"),
    ];
    match identity.layout_version {
        1 if identity.metadata_volume.is_none() => {}
        2 => expected.push((identity.metadata_volume?, "/velnor/init/metadata")),
        _ => return None,
    }
    Some(expected)
}

pub(super) fn capabilities_match(actual: &[String], expected: &[&str]) -> bool {
    if actual.len() != expected.len() {
        return false;
    }
    let mut normalized = Vec::with_capacity(actual.len());
    for capability in actual {
        let value = capability.strip_prefix("CAP_").unwrap_or(capability);
        if !expected.contains(&value) || normalized.contains(&value) {
            return false;
        }
        normalized.push(value);
    }
    expected
        .iter()
        .all(|capability| normalized.contains(capability))
}

pub(super) fn image_environment_matches(actual: &[String]) -> bool {
    actual == [IMAGE_PATH_ENV]
}

pub(crate) fn container_matches_claim_identity(
    inspected: &ContainerInspectResponse,
    claim: &StorageInitializationClaim,
    id: &str,
) -> bool {
    container_matches_identity(inspected, &InitializerIdentity::from_claim(claim), id)
}

pub(super) fn network_isolated(inspected: &ContainerInspectResponse) -> bool {
    let Some(settings) = inspected.network_settings.as_ref() else {
        return false;
    };
    if !settings.sandbox_id.as_deref().is_some_and(str::is_empty)
        || !settings.sandbox_key.as_deref().is_some_and(str::is_empty)
        || settings
            .ports
            .as_ref()
            .is_none_or(|ports| !ports.is_empty())
    {
        return false;
    }
    let Some(networks) = settings.networks.as_ref() else {
        return false;
    };
    if networks.len() != 1 {
        return false;
    }
    let Some(none) = networks.get("none") else {
        return false;
    };
    none.ipam_config.is_none()
        && none.links.as_ref().is_none_or(Vec::is_empty)
        && none.mac_address.as_deref().is_some_and(str::is_empty)
        && none.aliases.as_ref().is_none_or(Vec::is_empty)
        && none
            .driver_opts
            .as_ref()
            .is_none_or(std::collections::HashMap::is_empty)
        && none.network_id.as_deref().is_some_and(str::is_empty)
        && none.endpoint_id.as_deref().is_some_and(str::is_empty)
        && none.gateway.as_deref().is_some_and(str::is_empty)
        && none.ip_address.as_deref().is_some_and(str::is_empty)
        && none.ip_prefix_len == Some(0)
        && none.ipv6_gateway.as_deref().is_some_and(str::is_empty)
        && none
            .global_ipv6_address
            .as_deref()
            .is_some_and(str::is_empty)
        && none.global_ipv6_prefix_len == Some(0)
        && none.dns_names.as_ref().is_none_or(Vec::is_empty)
}

pub(super) fn engine_identity_matches(
    actual_id: &str,
    actual_root: &str,
    expected_id: &str,
    expected_root: &str,
) -> bool {
    !actual_id.is_empty()
        && !actual_root.is_empty()
        && actual_id == expected_id
        && actual_root == expected_root
}

pub(super) fn volume_identity_matches(
    actual_name: &str,
    labels: &std::collections::HashMap<String, String>,
    expected_name: &str,
    connection_id: &str,
    reservation_id: i64,
    operation_id: i64,
    role: &str,
) -> bool {
    actual_name == expected_name
        && labels.get("velnor.connection").map(String::as_str) == Some(connection_id)
        && labels.get("velnor.reservation").map(String::as_str)
            == Some(reservation_id.to_string().as_str())
        && labels.get("velnor.operation").map(String::as_str)
            == Some(operation_id.to_string().as_str())
        && labels.get("velnor.role").map(String::as_str) == Some(role)
}
