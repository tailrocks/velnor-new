//! Exact bounded Docker accounting for journal-owned worker pairs.

use bollard::Docker;
use bollard::errors::Error as DockerError;
use bollard::models::{ContainerInspectResponse, HostConfig};

use crate::docker_client::docker_deadline;
use crate::journal::Journal;
use crate::reconcile::IntentRow;
use crate::scale_set::EnsureError;
use crate::worker::{DockerResourceLimits, ResourceBudget};

use super::OccupiedResources;
use crate::launch::slot;

pub(super) async fn occupied(
    docker: &Docker,
    journal: &Journal,
    budget: ResourceBudget,
    ceiling: u32,
) -> Result<OccupiedResources, EnsureError> {
    let rows = journal
        .capacity_rows(ceiling)
        .await
        .map_err(|_| capacity_error("journal"))?;
    let mut total = OccupiedResources::default();
    for row in rows.iter().filter(|row| slot::holds(row)) {
        total.permits = total
            .permits
            .checked_add(1)
            .ok_or_else(|| capacity_error("capacity"))?;
        add_usage(&mut total, row_usage(docker, row, budget).await?)?;
    }
    Ok(total)
}

async fn row_usage(
    docker: &Docker,
    row: &IntentRow,
    budget: ResourceBudget,
) -> Result<OccupiedResources, EnsureError> {
    let volume = match row
        .worker_volume
        .as_deref()
        .filter(|value| !value.is_empty())
    {
        None if slot::idless_unattempted(row) => return configured_pair_usage(budget),
        Some(value) if valid_worker(value) => value,
        _ => return Err(capacity_error("worker ownership")),
    };
    if invalid_recorded_id(row.docker_id.as_deref()) || invalid_recorded_id(row.dind_id.as_deref())
    {
        return Err(capacity_error("worker ownership"));
    }
    let runner = inspect_role(
        docker,
        volume,
        "runner",
        row.docker_id.as_deref(),
        budget.runner(),
    )
    .await?;
    let dind = inspect_role(
        docker,
        volume,
        "dind",
        row.dind_id.as_deref(),
        budget.dind(),
    )
    .await?;
    let mut usage = OccupiedResources::default();
    add_usage(&mut usage, runner)?;
    add_usage(&mut usage, dind)?;
    Ok(usage)
}

async fn inspect_role(
    docker: &Docker,
    volume: &str,
    role: &str,
    recorded_id: Option<&str>,
    configured: DockerResourceLimits,
) -> Result<OccupiedResources, EnsureError> {
    let name = format!("{volume}-{role}");
    let response = docker_deadline(docker.inspect_container(&name, None))
        .await
        .map_err(|_| capacity_error("docker inspect"))?;
    match response {
        Ok(container) => inspected_usage(&container, &name, volume, role, recorded_id),
        Err(DockerError::DockerResponseServerError {
            status_code: 404, ..
        }) => missing_by_name(docker, &name, volume, role, recorded_id, configured).await,
        Err(_) => Err(capacity_error("docker inspect")),
    }
}

async fn missing_by_name(
    docker: &Docker,
    name: &str,
    volume: &str,
    role: &str,
    recorded_id: Option<&str>,
    configured: DockerResourceLimits,
) -> Result<OccupiedResources, EnsureError> {
    let Some(id) = recorded_id else {
        return configured_usage(configured);
    };
    match docker_deadline(docker.inspect_container(id, None))
        .await
        .map_err(|_| capacity_error("docker inspect"))?
    {
        Err(DockerError::DockerResponseServerError {
            status_code: 404, ..
        }) => configured_usage(configured),
        Ok(container) => inspected_usage(&container, name, volume, role, Some(id)),
        Err(_) => Err(capacity_error("docker inspect")),
    }
}

fn inspected_usage(
    container: &ContainerInspectResponse,
    name: &str,
    volume: &str,
    role: &str,
    recorded_id: Option<&str>,
) -> Result<OccupiedResources, EnsureError> {
    let id = container
        .id
        .as_deref()
        .filter(|id| valid_id(id))
        .ok_or_else(|| capacity_error("worker ownership"))?;
    if recorded_id.is_some_and(|recorded| recorded != id)
        || !container_name_matches(container, name)
        || !container_labels_match(container, volume, role)
    {
        return Err(capacity_error("worker ownership"));
    }
    resource_usage(
        container
            .host_config
            .as_ref()
            .ok_or_else(|| capacity_error("worker resources"))?,
    )
}

fn resource_usage(host: &HostConfig) -> Result<OccupiedResources, EnsureError> {
    let nano_cpus = positive(host.nano_cpus)?;
    let memory_bytes = positive(host.memory)?;
    let swap_bytes = positive(host.memory_swap)?;
    if memory_bytes != swap_bytes {
        return Err(capacity_error("worker resources"));
    }
    Ok(OccupiedResources {
        permits: 0,
        nano_cpus,
        memory_bytes,
    })
}

fn positive(value: Option<i64>) -> Result<u64, EnsureError> {
    value
        .filter(|item| *item > 0)
        .and_then(|item| u64::try_from(item).ok())
        .ok_or_else(|| capacity_error("worker resources"))
}

fn configured_usage(limits: DockerResourceLimits) -> Result<OccupiedResources, EnsureError> {
    Ok(OccupiedResources {
        permits: 0,
        nano_cpus: u64::try_from(limits.nano_cpus)
            .ok()
            .filter(|value| *value > 0)
            .ok_or_else(|| capacity_error("worker resources"))?,
        memory_bytes: u64::try_from(limits.memory_bytes)
            .ok()
            .filter(|value| *value > 0)
            .ok_or_else(|| capacity_error("worker resources"))?,
    })
}

fn configured_pair_usage(budget: ResourceBudget) -> Result<OccupiedResources, EnsureError> {
    let pair = budget.pair();
    Ok(OccupiedResources {
        permits: 0,
        nano_cpus: pair
            .cpu_millicores
            .checked_mul(super::NANO_CPUS_PER_MILLICORE)
            .filter(|value| *value > 0)
            .ok_or_else(|| capacity_error("worker resources"))?,
        memory_bytes: (pair.memory_bytes > 0)
            .then_some(pair.memory_bytes)
            .ok_or_else(|| capacity_error("worker resources"))?,
    })
}

fn add_usage(total: &mut OccupiedResources, row: OccupiedResources) -> Result<(), EnsureError> {
    total.nano_cpus = total
        .nano_cpus
        .checked_add(row.nano_cpus)
        .ok_or_else(|| capacity_error("capacity"))?;
    total.memory_bytes = total
        .memory_bytes
        .checked_add(row.memory_bytes)
        .ok_or_else(|| capacity_error("capacity"))?;
    Ok(())
}

fn container_name_matches(container: &ContainerInspectResponse, expected: &str) -> bool {
    container
        .name
        .as_deref()
        .map(|name| name.strip_prefix('/').unwrap_or(name))
        == Some(expected)
}

fn container_labels_match(container: &ContainerInspectResponse, volume: &str, role: &str) -> bool {
    let Some(labels) = container
        .config
        .as_ref()
        .and_then(|config| config.labels.as_ref())
    else {
        return false;
    };
    [
        ("velnor.volume", volume),
        ("velnor.worker", volume),
        ("velnor.role", role),
    ]
    .iter()
    .all(|(key, value)| labels.get(*key).map(String::as_str) == Some(*value))
        && labels
            .keys()
            .filter(|key| key.starts_with("velnor."))
            .count()
            == 3
}

fn valid_worker(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'.' | b'-'))
}

fn valid_id(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn invalid_recorded_id(value: Option<&str>) -> bool {
    value.is_some_and(|id| !valid_id(id))
}

const fn capacity_error(step: &'static str) -> EnsureError {
    EnsureError::Unexpected { status: 0, step }
}
