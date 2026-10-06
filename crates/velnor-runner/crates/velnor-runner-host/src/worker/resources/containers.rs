//! Idempotent container create checks.

use std::collections::{HashMap, HashSet};
use std::time::Duration;

use bollard::Docker;
use bollard::models::{ContainerSummaryStateEnum as ContainerState, HostConfigCgroupnsModeEnum};
use bollard::query_parameters::ListContainersOptionsBuilder;
use tokio::time::timeout;

use super::super::mounts::{label_map, mount_source};
use super::super::{
    CreateProjection, DIND_ENTRYPOINT, DIND_IMAGE, ResourceBudget, dind_create_for_identity,
    identity_labels_match, join_dind_net, launch_identity_labels_match, runner_create_for_identity,
};
use super::confirmed_not_found;
use crate::error::HostError;
use crate::launch_identity::LaunchIdentity;
use crate::stage::ContainerRecord;

mod environment;
use environment::environment_matches;
#[path = "containers_limits.rs"]
mod limits;
use limits::resource_limits_match;

const DOCKER_CALL_TIMEOUT: Duration = Duration::from_secs(5);

pub(crate) async fn refuse_existing(
    docker: &Docker,
    spec: &CreateProjection,
) -> Result<(), HostError> {
    let name = spec.name.as_str();
    let expected = label_map(&spec.labels)?.ok_or(HostError::Ownership)?;
    let launch = expected.get("velnor.launch").ok_or(HostError::Ownership)?;
    let instance = expected
        .get("velnor.instance")
        .ok_or(HostError::Ownership)?;
    let role = expected.get("velnor.role").ok_or(HostError::Ownership)?;
    let mut filters = HashMap::new();
    filters.insert(
        "label".to_owned(),
        vec![
            format!("velnor.launch={launch}"),
            format!("velnor.instance={instance}"),
        ],
    );
    let options = ListContainersOptionsBuilder::default()
        .all(true)
        .filters(&filters)
        .build();
    let rows = timeout(DOCKER_CALL_TIMEOUT, docker.list_containers(Some(options)))
        .await
        .map_err(|_| HostError::DockerTimeout)?
        .map_err(|_| HostError::Docker)?;
    let mut dind = Vec::new();
    let mut runner = Vec::new();
    for row in rows {
        let labels = row.labels.as_ref().ok_or(HostError::Ownership)?;
        match validate_existing_row(&expected, labels)? {
            "dind" => dind.push(row),
            "runner" => runner.push(row),
            _ => return Err(HostError::Ownership),
        }
    }
    reject_existing_role(docker, spec, role, &dind, &runner).await?;
    match inspect_container(docker, name).await? {
        Some(_) => Err(HostError::Ownership),
        None => Ok(()),
    }
}

async fn reject_existing_role(
    docker: &Docker,
    spec: &CreateProjection,
    role: &str,
    dind: &[bollard::models::ContainerSummary],
    runner: &[bollard::models::ContainerSummary],
) -> Result<(), HostError> {
    match (role, dind, runner) {
        ("dind", [], []) => Ok(()),
        ("dind", [dind_row], []) => verify_existing(docker, spec, dind_row).await,
        ("runner", [dind_row], []) => verify_dind(docker, spec, dind_row).await,
        ("dind", [], [_]) => Err(HostError::Ownership),
        ("runner", [], [runner_row]) => verify_existing(docker, spec, runner_row).await,
        _ => Err(HostError::Ownership),
    }
}

async fn verify_dind(
    docker: &Docker,
    runner: &CreateProjection,
    row: &bollard::models::ContainerSummary,
) -> Result<(), HostError> {
    let expected = dind_projection(runner)?;
    let id = row.id.as_deref().ok_or(HostError::Ownership)?;
    let found = inspect_container(docker, id)
        .await?
        .ok_or(HostError::Ownership)?;
    let named = expected.name.as_str();
    let by_name = inspect_container(docker, named)
        .await?
        .ok_or(HostError::Ownership)?;
    if found.id != by_name.id
        || !topology_matches(&expected, &found)?
        || found.state.as_ref().and_then(|state| state.running) != Some(true)
    {
        return Err(HostError::Ownership);
    }
    Ok(())
}

pub(super) async fn list_launch(
    docker: &Docker,
    identity: &LaunchIdentity,
) -> Result<Vec<ContainerRecord>, HostError> {
    let mut filters = HashMap::new();
    filters.insert(
        "label".to_owned(),
        vec![
            "velnor.product=velnor".to_owned(),
            format!("velnor.instance={}", identity.instance_id()),
            format!("velnor.launch={}", identity.launch_id()),
            format!("velnor.engine={}", identity.engine_id()),
        ],
    );
    let options = ListContainersOptionsBuilder::default()
        .all(true)
        .filters(&filters)
        .build();
    let rows = timeout(DOCKER_CALL_TIMEOUT, docker.list_containers(Some(options)))
        .await
        .map_err(|_| HostError::DockerTimeout)?
        .map_err(|_| HostError::Docker)?;
    rows.into_iter()
        .map(|row| {
            Ok(ContainerRecord {
                id: row.id.ok_or(HostError::Ownership)?,
                labels: row.labels.ok_or(HostError::Ownership)?,
                running: row.state.map(|state| state == ContainerState::RUNNING),
            })
        })
        .collect()
}

pub(super) async fn verify_container(
    docker: &Docker,
    identity: &LaunchIdentity,
    role: &str,
    id: &str,
    dind_id: Option<&str>,
    resource_budget: Option<ResourceBudget>,
    require_running: bool,
) -> Result<ContainerRecord, HostError> {
    if !container_id(id) {
        return Err(HostError::Ownership);
    }
    let expected = match role {
        "dind" if dind_id.is_none() => {
            let mut expected = dind_create_for_identity(identity)?;
            expected.resource_budget = resource_budget;
            expected
        }
        "runner" => {
            let mut runner = runner_create_for_identity(identity, None)?;
            runner.resource_budget = resource_budget;
            join_dind_net(runner, dind_id.ok_or(HostError::Ownership)?)?
        }
        _ => return Err(HostError::Ownership),
    };
    let name = expected.name.as_str();
    let found = inspect_container(docker, id)
        .await?
        .ok_or(HostError::Ownership)?;
    let named = inspect_container(docker, name)
        .await?
        .ok_or(HostError::Ownership)?;
    let labels = found
        .config
        .as_ref()
        .and_then(|config| config.labels.clone())
        .ok_or(HostError::Ownership)?;
    let found_id = found.id.as_deref().ok_or(HostError::Ownership)?;
    let running = found.state.as_ref().and_then(|state| state.running);
    if found_id != id
        || named.id.as_deref() != Some(id)
        || !topology_matches(&expected, &found)?
        || (require_running && running != Some(true))
    {
        return Err(HostError::Ownership);
    }
    Ok(ContainerRecord {
        id: id.to_owned(),
        labels,
        running,
    })
}

async fn verify_existing(
    docker: &Docker,
    expected: &CreateProjection,
    row: &bollard::models::ContainerSummary,
) -> Result<(), HostError> {
    let id = row.id.as_deref().ok_or(HostError::Ownership)?;
    let found = inspect_container(docker, id)
        .await?
        .ok_or(HostError::Ownership)?;
    if !topology_matches(expected, &found)? {
        return Err(HostError::Ownership);
    }
    Err(HostError::Ownership)
}

fn same_launch(expected: &HashMap<String, String>, actual: &HashMap<String, String>) -> bool {
    launch_identity_labels_match(expected, actual)
}

fn validate_existing_row(
    expected: &HashMap<String, String>,
    actual: &HashMap<String, String>,
) -> Result<&'static str, HostError> {
    if !same_launch(expected, actual) {
        return Err(HostError::Ownership);
    }
    match actual.get("velnor.role").map(String::as_str) {
        Some("dind") => Ok("dind"),
        Some("runner") => Ok("runner"),
        _ => Err(HostError::Ownership),
    }
}

fn dind_projection(runner: &CreateProjection) -> Result<CreateProjection, HostError> {
    let mut labels = runner.labels.clone();
    let role = labels
        .iter_mut()
        .find(|label| label.starts_with("velnor.role="))
        .ok_or(HostError::Ownership)?;
    role.replace_range(.., "velnor.role=dind");
    let volume = runner
        .labels
        .iter()
        .find_map(|label| label.strip_prefix("velnor.volume="))
        .ok_or(HostError::Ownership)?;
    let name = runner
        .name
        .strip_prefix("velnor-runner-")
        .map(|launch| format!("velnor-dind-{launch}"))
        .ok_or(HostError::Ownership)?;
    let mut mounts = runner.mounts.clone();
    mounts.push(crate::docker_spec::Mount {
        source: format!("volume:{volume}-docker"),
        target: "/var/lib/docker".to_owned(),
    });
    Ok(CreateProjection {
        name,
        image: DIND_IMAGE.to_owned(),
        platform: runner.platform.clone(),

        env: Vec::new(),
        cmd: Vec::new(),
        entrypoint: DIND_ENTRYPOINT
            .iter()
            .map(|item| (*item).to_owned())
            .collect(),
        user: None,
        working_dir: None,
        labels,
        mounts,
        bind_mounts: Vec::new(),
        privileged: true,
        open_stdin: false,
        network_mode: None,
        resource_budget: runner.resource_budget,
    })
}

async fn inspect_container(
    docker: &Docker,
    id_or_name: &str,
) -> Result<Option<bollard::models::ContainerInspectResponse>, HostError> {
    let response = timeout(
        DOCKER_CALL_TIMEOUT,
        docker.inspect_container(id_or_name, None),
    )
    .await
    .map_err(|_| HostError::DockerTimeout)?;
    match response {
        Ok(found) => Ok(Some(found)),
        Err(error) if confirmed_not_found(&error) => Ok(None),
        Err(_) => Err(HostError::Docker),
    }
}

fn topology_matches(
    spec: &CreateProjection,
    found: &bollard::models::ContainerInspectResponse,
) -> Result<bool, HostError> {
    let Some(config) = found.config.as_ref() else {
        return Ok(false);
    };
    let Some(host) = found.host_config.as_ref() else {
        return Ok(false);
    };
    let labels = label_map(&spec.labels)?.ok_or(HostError::Ownership)?;
    if config.image.as_deref() != Some(spec.image.as_str())
        || !inspect_labels_match(&labels, found)
        || !execution_matches(spec, config)
        || !environment_matches(spec, config)?
        || host.privileged != Some(spec.privileged)
        || host.cgroupns_mode != Some(HostConfigCgroupnsModeEnum::PRIVATE)
        || !resource_limits_match(spec, host)
        || !network_matches(spec.network_mode.as_deref(), host.network_mode.as_deref())
    {
        return Ok(false);
    }
    Ok(expected_mounts(spec)? == observed_mounts(found)?)
}

fn execution_matches(spec: &CreateProjection, config: &bollard::models::ContainerConfig) -> bool {
    config.cmd.as_deref().unwrap_or_default() == spec.cmd.as_slice()
        && config.entrypoint.as_deref() == Some(spec.entrypoint.as_slice())
        && image_value_matches(spec.user.as_deref(), config.user.as_deref())
        && image_value_matches(spec.working_dir.as_deref(), config.working_dir.as_deref())
        && config.attach_stdin == Some(spec.open_stdin)
        && config.open_stdin == Some(spec.open_stdin)
        && config.stdin_once == Some(spec.open_stdin)
        && !config.tty.unwrap_or(false)
}

fn image_value_matches(expected: Option<&str>, actual: Option<&str>) -> bool {
    expected == actual.filter(|value| !value.is_empty())
}

fn inspect_labels_match(
    expected: &HashMap<String, String>,
    found: &bollard::models::ContainerInspectResponse,
) -> bool {
    let Some(actual) = found
        .config
        .as_ref()
        .and_then(|config| config.labels.as_ref())
    else {
        return false;
    };
    identity_labels_match(expected, actual)
}

fn network_matches(expected: Option<&str>, actual: Option<&str>) -> bool {
    match expected {
        Some(mode) => actual == Some(mode),
        None => actual.is_none_or(|mode| mode.is_empty() || mode == "default"),
    }
}

fn container_id(id: &str) -> bool {
    (12..=64).contains(&id.len()) && id.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn expected_mounts(
    spec: &CreateProjection,
) -> Result<HashSet<(String, String, String, bool)>, HostError> {
    let mut mounts = HashSet::with_capacity(spec.mounts.len() + spec.bind_mounts.len());
    for mount in &spec.mounts {
        let (_, source) = mount_source(&mount.source)?;
        mounts.insert(("volume".to_owned(), source, mount.target.clone(), true));
    }
    for mount in &spec.bind_mounts {
        mounts.insert((
            "bind".to_owned(),
            mount.source.clone(),
            mount.target.clone(),
            !mount.read_only,
        ));
    }
    Ok(mounts)
}

fn observed_mounts(
    found: &bollard::models::ContainerInspectResponse,
) -> Result<HashSet<(String, String, String, bool)>, HostError> {
    let records = found.mounts.as_ref().ok_or(HostError::Ownership)?;
    let mut mounts = HashSet::with_capacity(records.len());
    for mount in records {
        let typ = mount.typ.as_ref().ok_or(HostError::Ownership)?;
        let source = match typ.as_str() {
            "volume" => mount.name.as_ref(),
            "bind" => mount.source.as_ref(),
            _ => return Err(HostError::Ownership),
        }
        .ok_or(HostError::Ownership)?;
        mounts.insert((
            typ.clone(),
            source.clone(),
            mount.destination.clone().ok_or(HostError::Ownership)?,
            mount.rw.ok_or(HostError::Ownership)?,
        ));
    }
    Ok(mounts)
}

#[cfg(test)]
#[path = "containers_tests.rs"]
mod tests;
