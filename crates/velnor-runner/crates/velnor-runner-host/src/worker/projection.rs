//! Durable launch projections for runner and `DinD` containers.

#[cfg(test)]
use std::collections::HashMap;
#[cfg(test)]
use std::path::Path;

#[cfg(test)]
use crate::docker_spec::{Mount, runner_plan};
#[cfg(test)]
use crate::error::HostError;
#[cfg(test)]
use crate::launch_identity::LaunchIdentity;

#[cfg(test)]
use super::{BindMount, CreateProjection, DIND_ENTRYPOINT, DIND_IMAGE, runner_create};

/// Private `DinD` create. Privilege is not a flag on the runner plan.
///
/// Mounts include private socket, workspace, and VFS data volumes.
///
/// # Errors
///
/// Returns [`HostError::ForbiddenMount`] when the identity has an invalid volume.
#[cfg(test)]
pub(crate) fn dind_create_for_identity(
    identity: &LaunchIdentity,
) -> Result<CreateProjection, HostError> {
    let resource_budget = super::resource_budget::test_resource_budget()?;
    let runner = runner_plan(identity.private_volume())?;
    let mut mounts = runner.mounts;
    mounts.push(Mount {
        source: format!("volume:{}-docker", identity.private_volume()),
        target: "/var/lib/docker".to_owned(),
    });
    Ok(CreateProjection {
        name: container_name(identity, "dind"),
        image: DIND_IMAGE.to_owned(),
        platform: runner.platform,
        env: Vec::new(),
        cmd: Vec::new(),
        entrypoint: DIND_ENTRYPOINT
            .iter()
            .map(|item| (*item).to_owned())
            .collect(),
        user: None,
        working_dir: None,
        labels: container_labels(identity, "dind"),
        mounts,
        bind_mounts: Vec::new(),
        privileged: true,
        open_stdin: false,
        network_mode: None,
        resource_budget: Some(resource_budget),
    })
}

/// Build one runner projection with its launch labels and optional action cache.
///
/// # Errors
///
/// Returns [`HostError::Path`] when the cache path is not absolute or Unicode.
/// Returns runner-plan errors from [`runner_create`].
#[cfg(test)]
pub(crate) fn runner_create_for_identity(
    identity: &LaunchIdentity,
    archive_cache_path: Option<&Path>,
) -> Result<CreateProjection, HostError> {
    let resource_budget = super::resource_budget::test_resource_budget()?;
    let plan = runner_plan(identity.private_volume())?;
    let mut spec = runner_create(&plan, resource_budget)?;
    spec.name = container_name(identity, "runner");
    spec.labels = container_labels(identity, "runner");
    if let Some(path) = archive_cache_path {
        if !path.is_absolute() {
            return Err(HostError::Path);
        }
        let source = path.to_str().ok_or(HostError::Path)?;
        spec.env
            .push("ACTIONS_RUNNER_ACTION_ARCHIVE_CACHE=/opt/velnor/action-archives".to_owned());
        spec.bind_mounts.push(BindMount {
            source: source.to_owned(),
            target: "/opt/velnor/action-archives".to_owned(),
            read_only: true,
        });
    }
    Ok(spec)
}

#[cfg(test)]
pub(crate) fn container_name(identity: &LaunchIdentity, role: &str) -> String {
    format!("velnor-{role}-{}", identity.launch_id())
}

#[cfg(test)]
pub(crate) fn container_labels(identity: &LaunchIdentity, role: &str) -> Vec<String> {
    vec![
        "velnor.product=velnor".to_owned(),
        format!("velnor.instance={}", identity.instance_id()),
        format!("velnor.launch={}", identity.launch_id()),
        format!("velnor.engine={}", identity.engine_id()),
        format!("velnor.role={role}"),
        format!("velnor.volume={}", identity.private_volume()),
    ]
}

/// Match required Velnor labels and reject extra Velnor keys.
///
/// Docker can include image labels in inspected container configuration. Those labels
/// do not define Velnor ownership and may be present alongside the exact launch labels.
#[cfg(test)]
pub(crate) fn identity_labels_match(
    expected: &HashMap<String, String>,
    actual: &HashMap<String, String>,
) -> bool {
    expected
        .iter()
        .all(|(key, value)| actual.get(key) == Some(value))
        && actual
            .keys()
            .filter(|key| key.starts_with("velnor."))
            .all(|key| expected.contains_key(key))
}

/// Match launch identity across the `DinD` and runner roles.
#[cfg(test)]
pub(crate) fn launch_identity_labels_match(
    expected: &HashMap<String, String>,
    actual: &HashMap<String, String>,
) -> bool {
    expected
        .iter()
        .filter(|(key, _)| key.as_str() != "velnor.role")
        .all(|(key, value)| actual.get(key) == Some(value))
        && matches!(
            actual.get("velnor.role").map(String::as_str),
            Some("dind" | "runner")
        )
        && actual
            .keys()
            .filter(|key| key.starts_with("velnor.") && key.as_str() != "velnor.role")
            .all(|key| expected.contains_key(key))
}
