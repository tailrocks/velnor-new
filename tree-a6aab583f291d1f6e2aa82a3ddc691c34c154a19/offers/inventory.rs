//! Complete Docker inventory gate for session effects.

use std::time::Instant;

use tokio::time::{Instant as TokioInstant, timeout_at};
use velnor_runner_host::worker::{
    OwnedDockerResource, OwnedDockerResourceKind, list_owned_docker_resources_until,
    worker_volume_names,
};
use velnor_runner_host::{IntentRow, IntentState};
use velnor_runner_journal::journal::{Journal, LaunchEffectState, RunnerStartIntent};
use velnor_runner_launch_slot::holds;

use crate::linux::{LinuxLaunchContext, session::ActiveSession};

/// Return free globally reserved slots only after a complete, row-matched inventory.
pub(super) async fn free_slots(
    context: &LinuxLaunchContext,
    journal: &Journal,
    active: &ActiveSession,
    deadline: Instant,
) -> Option<u32> {
    if Instant::now() >= deadline {
        return None;
    }
    let tokio_deadline = TokioInstant::from_std(deadline);
    let inventory = timeout_at(
        tokio_deadline,
        list_owned_docker_resources_until(&context.docker_endpoint, tokio_deadline),
    )
    .await
    .ok()?
    .ok()?;
    let rows = timeout_at(tokio_deadline, journal.rows())
        .await
        .ok()?
        .ok()?;
    if Instant::now() >= deadline
        || !non_launch_rows_resolved(&rows, active.intent_id)
        || !inventory_matches_rows(&inventory, &rows)
    {
        return None;
    }
    let occupied = rows
        .iter()
        .filter(|row| row.kind == "launch" && holds(row))
        .count();
    let maximum = usize::try_from(context.max_jobs().get()).ok()?;
    u32::try_from(maximum.saturating_sub(occupied)).ok()
}

fn inventory_matches_rows(resources: &[OwnedDockerResource], rows: &[IntentRow]) -> bool {
    if resources.iter().any(|resource| {
        let mut matching = rows.iter().filter(|row| {
            row.kind == "launch"
                && holds(row)
                && row.worker_volume.as_deref() == Some(resource.worker.as_str())
                && resource_matches_row(resource, row)
        });
        matching.next().is_none() || matching.next().is_some()
    }) {
        return false;
    }
    rows.iter()
        .filter(|row| row.kind == "launch" && holds(row))
        .all(|row| held_generation_is_complete(row, resources))
}

fn non_launch_rows_resolved(rows: &[IntentRow], active_session_id: i64) -> bool {
    rows.iter().all(|row| {
        if row.kind == "launch" {
            return true;
        }
        if row.id == active_session_id {
            return row.kind == "scale-set-session"
                && row.state == IntentState::Pending
                && row.launch_effect == LaunchEffectState::MayHaveEffect;
        }
        row.cleanup_proven
            || (row.state == IntentState::Failed
                && row.launch_effect == LaunchEffectState::DefiniteNoEffect)
            || (row.state == IntentState::Done
                && (row.kind == "scale-set-session"
                    || row.launch_effect == LaunchEffectState::MayHaveEffect))
    })
}

fn held_generation_is_complete(row: &IntentRow, resources: &[OwnedDockerResource]) -> bool {
    if row.state != IntentState::Done
        || row.launch_effect != LaunchEffectState::MayHaveEffect
        || row.runner_start_intent != RunnerStartIntent::MayHaveStarted
    {
        return false;
    }
    let (Some(worker), Some(runner), Some(dind), Some(network_id), Some(network_name)) = (
        row.worker_volume.as_deref(),
        row.docker_id.as_deref(),
        row.dind_id.as_deref(),
        row.outer_network_id.as_deref(),
        row.outer_network_name.as_deref(),
    ) else {
        return false;
    };
    let expected_volumes = [
        (worker.to_owned(), "socket"),
        (format!("{worker}-work"), "work"),
        (format!("{worker}-externals"), "externals"),
        (format!("{worker}-docker"), "dind-data"),
        (format!("{worker}-home"), "home-state"),
        (format!("{worker}-tmp"), "runner-temp"),
    ];
    let exact = |kind, id, role| {
        resources.iter().any(|resource| {
            resource.kind == kind
                && resource.worker == worker
                && resource.role == role
                && resource.id_or_name == id
        })
    };
    exact(OwnedDockerResourceKind::Container, runner, "runner")
        && exact(OwnedDockerResourceKind::Container, dind, "dind")
        && resources.iter().any(|resource| {
            resource.kind == OwnedDockerResourceKind::Network
                && resource.worker == worker
                && resource.role == "outer-network"
                && resource.id_or_name == network_id
                && resource.names.iter().any(|name| name == network_name)
        })
        && expected_volumes.iter().all(|(name, role)| {
            resources.iter().any(|resource| {
                resource.kind == OwnedDockerResourceKind::Volume
                    && resource.worker == worker
                    && resource.id_or_name == *name
                    && resource.role == *role
            })
        })
}

fn resource_matches_row(resource: &OwnedDockerResource, row: &IntentRow) -> bool {
    if resource.labels.get("velnor.worker").map(String::as_str) != Some(resource.worker.as_str())
        || resource.labels.get("velnor.role").map(String::as_str) != Some(&resource.role)
        || matches!(
            resource.kind,
            OwnedDockerResourceKind::Container | OwnedDockerResourceKind::Network
        ) && resource.labels.get("velnor.volume").map(String::as_str)
            != Some(resource.worker.as_str())
    {
        return false;
    }
    match resource.kind {
        OwnedDockerResourceKind::Container => match resource.role.as_str() {
            "runner" => row.docker_id.as_deref() == Some(resource.id_or_name.as_str()),
            "dind" => row.dind_id.as_deref() == Some(resource.id_or_name.as_str()),
            _ => false,
        },
        OwnedDockerResourceKind::Network => {
            resource.role == "outer-network"
                && row.outer_network_id.as_deref() == Some(resource.id_or_name.as_str())
                && row
                    .outer_network_name
                    .as_ref()
                    .is_some_and(|name| resource.names.iter().any(|observed| observed == name))
        }
        OwnedDockerResourceKind::Volume => {
            let Ok(names) = worker_volume_names(&resource.worker) else {
                return false;
            };
            names.iter().any(|name| name == &resource.id_or_name)
                && volume_role(&resource.id_or_name, &resource.worker)
                    == Some(resource.role.as_str())
        }
    }
}

fn volume_role(name: &str, worker: &str) -> Option<&'static str> {
    if name == worker {
        Some("socket")
    } else if name == format!("{worker}-work") {
        Some("work")
    } else if name == format!("{worker}-externals") {
        Some("externals")
    } else if name == format!("{worker}-docker") {
        Some("dind-data")
    } else if name == format!("{worker}-home") {
        Some("home-state")
    } else if name == format!("{worker}-tmp") {
        Some("runner-temp")
    } else {
        None
    }
}

#[cfg(test)]
#[path = "inventory_tests.rs"]
mod tests;
