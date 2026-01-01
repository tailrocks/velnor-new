//! Exact same-engine inspection predicates for probe execution and cleanup.

use bollard::{
    Docker,
    errors::Error as DockerError,
    models::{ContainerInspectResponse, ContainerStateStatusEnum},
};

use crate::error::HostError;

use super::lifecycle::Deadline;
use super::projection::ProbeProjection;

pub(super) async fn by_reference(
    docker: &Docker,
    reference: &str,
    deadline: &Deadline,
) -> Result<Option<ContainerInspectResponse>, HostError> {
    match deadline
        .docker(docker.inspect_container(reference, None))
        .await?
    {
        Ok(response) => Ok(Some(response)),
        Err(DockerError::DockerResponseServerError {
            status_code: 404, ..
        }) => Ok(None),
        Err(_) => Err(HostError::Docker),
    }
}

pub(super) fn matches(
    projection: &ProbeProjection,
    response: &ContainerInspectResponse,
    container_id: &str,
) -> bool {
    let name = format!("/{}", projection.name);
    response.id.as_deref() == Some(container_id)
        && response.name.as_deref() == Some(name.as_str())
        && response.image.as_deref() == Some(projection.image.runtime_id())
        && response.restart_count == Some(0)
        && response.path.as_deref() == Some("/velnor/resource-probe")
        && response.args.as_ref().is_none_or(Vec::is_empty)
        && config_matches(projection, response, container_id)
        && host_matches(projection, response)
        && mounts_match(projection, response)
}

pub(super) fn exited_successfully(response: &ContainerInspectResponse) -> bool {
    response.state.as_ref().is_some_and(|state| {
        state.status == Some(ContainerStateStatusEnum::EXITED)
            && state.running == Some(false)
            && state.exit_code == Some(0)
            && state.oom_killed == Some(false)
    })
}

pub(super) fn running(response: &ContainerInspectResponse) -> bool {
    response.state.as_ref().is_some_and(|state| {
        matches!(
            state.status,
            Some(
                ContainerStateStatusEnum::RUNNING
                    | ContainerStateStatusEnum::PAUSED
                    | ContainerStateStatusEnum::RESTARTING
                    | ContainerStateStatusEnum::STOPPING
            )
        )
    })
}

pub(super) fn created(response: &ContainerInspectResponse) -> bool {
    response.state.as_ref().is_some_and(|state| {
        state.status == Some(ContainerStateStatusEnum::CREATED) && state.running == Some(false)
    })
}

pub(super) fn exited(response: &ContainerInspectResponse) -> bool {
    response.state.as_ref().is_some_and(|state| {
        matches!(
            state.status,
            Some(ContainerStateStatusEnum::EXITED | ContainerStateStatusEnum::DEAD)
        ) && state.running == Some(false)
    })
}

pub(super) fn created_or_exited(response: &ContainerInspectResponse) -> bool {
    created(response) || exited(response)
}

fn config_matches(
    projection: &ProbeProjection,
    response: &ContainerInspectResponse,
    container_id: &str,
) -> bool {
    let Some(mut actual) = response.config.clone() else {
        return false;
    };
    let Some(expected) = super::projection::expected_container_config(projection) else {
        return false;
    };
    normalize_docker_defaults(&mut actual, container_id) && actual == expected
}

fn normalize_docker_defaults(actual: &mut bollard::models::ContainerConfig, id: &str) -> bool {
    let Some(short_id) = id.get(..12) else {
        return false;
    };
    if actual
        .hostname
        .as_deref()
        .is_some_and(|hostname| !hostname.is_empty() && hostname != short_id)
        || actual
            .domainname
            .as_deref()
            .is_some_and(|domain| !domain.is_empty())
    {
        return false;
    }
    actual.hostname = None;
    actual.domainname = None;
    if actual.cmd.as_ref().is_some_and(Vec::is_empty) {
        actual.cmd = None;
    }
    if actual.network_disabled == Some(false) {
        return false;
    }
    if actual.network_disabled.is_none() {
        // Engine 29 omits this inspect field; host_matches still requires NetworkMode=none.
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
    true
}

fn host_matches(projection: &ProbeProjection, response: &ContainerInspectResponse) -> bool {
    let Some(actual) = response.host_config.as_ref() else {
        return false;
    };
    actual.auto_remove == Some(false)
        && actual.privileged == Some(false)
        && actual.readonly_rootfs == Some(true)
        && actual.network_mode.as_deref() == Some("none")
        && actual.cap_drop.as_deref() == Some(["ALL".to_owned()].as_slice())
        && actual.cap_add.as_ref().is_none_or(Vec::is_empty)
        && actual.security_opt.as_deref() == Some(["no-new-privileges:true".to_owned()].as_slice())
        && actual.devices.as_ref().is_none_or(Vec::is_empty)
        && actual.device_requests.as_ref().is_none_or(Vec::is_empty)
        && actual.binds.as_ref().is_none_or(Vec::is_empty)
        && actual.volumes_from.as_ref().is_none_or(Vec::is_empty)
        && actual
            .port_bindings
            .as_ref()
            .is_none_or(std::collections::HashMap::is_empty)
        && actual.restart_policy.as_ref().is_none_or(|policy| {
            policy.name.is_none_or(|name| {
                matches!(
                    name,
                    bollard::models::RestartPolicyNameEnum::EMPTY
                        | bollard::models::RestartPolicyNameEnum::NO
                )
            }) && policy.maximum_retry_count.is_none_or(|count| count == 0)
        })
        && actual.mounts.as_ref().is_some_and(|mounts| {
            mounts.len() == 1
                && mounts[0].source.as_deref() == Some(projection.root.path())
                && mounts[0].target.as_deref() == Some("/velnor/docker-root")
                && mounts[0]
                    .typ
                    .as_ref()
                    .is_some_and(|kind| kind.to_string() == "bind")
                && mounts[0].read_only == Some(true)
        })
}

fn mounts_match(projection: &ProbeProjection, response: &ContainerInspectResponse) -> bool {
    response.mounts.as_ref().is_some_and(|mounts| {
        mounts.len() == 1
            && mounts[0].source.as_deref() == Some(projection.root.path())
            && mounts[0].destination.as_deref() == Some("/velnor/docker-root")
            && mounts[0].typ.as_deref() == Some("bind")
            && mounts[0].rw == Some(false)
    })
}
