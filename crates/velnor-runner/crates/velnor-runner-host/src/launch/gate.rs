// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;

use crate::config::HostConfig;
use crate::error::HostError;
use crate::host_controller::connection_pat;
use crate::journal::Journal;
use crate::keychain;
use crate::reconcile::{IntentRow, Reconcile, before_advertise, occupies};
use crate::scale_set::{EnsureError, observe_scale_set_runners};
use bollard::Docker;

use super::inspect::container_running;

#[must_use]
pub(crate) fn gate_line(decision: &Reconcile) -> String {
    match decision {
        Reconcile::Hold { adopt, occupied } => {
            let adopt_count = adopt.len();
            format!("hold occupied={occupied} adopt={adopt_count}")
        }
        Reconcile::Advertise { occupied } => format!("advertise occupied={occupied}"),
    }
}

pub(crate) async fn reconcile_gate(
    journal: &Journal,
    docker: &Docker,
    config: &HostConfig,
) -> Result<Reconcile, EnsureError> {
    let rows = journal.rows().await.map_err(map_journal)?;
    let observed_docker = running_ids(&rows, docker).await?;
    let observed_github = observe_github(&rows, config);
    Ok(decide(&rows, &observed_docker, observed_github))
}

/// Combines the Docker and GitHub observations. When GitHub could not be
/// observed the gate stays closed: capacity is never released on a guess.
fn decide(
    rows: &[IntentRow],
    observed_docker: &[String],
    observed_github: Result<Vec<String>, EnsureError>,
) -> Reconcile {
    let Ok(observed_github) = observed_github else {
        let occupied = rows.iter().filter(|row| occupies(row)).count();
        return Reconcile::Hold {
            adopt: Vec::new(),
            occupied,
        };
    };
    before_advertise(
        rows,
        &str_refs(observed_docker),
        &str_refs(&observed_github),
        &owned_ids(rows),
    )
}

/// Scale sets whose runners must be listed before any row may be released.
/// A row that names a runner but no scale set cannot be observed at all.
fn scale_sets_to_observe(rows: &[IntentRow]) -> Result<BTreeSet<i64>, EnsureError> {
    let mut sets = BTreeSet::new();
    for row in rows.iter().filter(|row| !row.cleanup_proven) {
        if row.github_runner_id.as_deref().is_none_or(str::is_empty) {
            continue;
        }
        sets.insert(row.scale_set_id.ok_or(EnsureError::Uncertain)?);
    }
    Ok(sets)
}

fn observe_github(rows: &[IntentRow], config: &HostConfig) -> Result<Vec<String>, EnsureError> {
    let mut pending = scale_sets_to_observe(rows)?;
    let mut observed = Vec::new();
    for binding in config.connections() {
        if pending.is_empty() {
            break;
        }
        let Ok(credential) = keychain::load_secret(
            binding.credential_ref.service(),
            binding.credential_ref.account(),
        ) else {
            continue;
        };
        let Ok(pat) = connection_pat(&credential) else {
            continue;
        };
        for scale_set_id in pending.clone() {
            if let Some(runners) = observe_scale_set_runners(pat, binding, scale_set_id)? {
                observed.extend(runners);
                pending.remove(&scale_set_id);
            }
        }
    }
    if pending.is_empty() {
        Ok(observed)
    } else {
        Err(EnsureError::Uncertain)
    }
}

fn str_refs(ids: &[String]) -> Vec<&str> {
    ids.iter().map(String::as_str).collect()
}

fn owned_ids(rows: &[IntentRow]) -> Vec<&str> {
    rows.iter()
        .filter_map(|row| row.docker_id.as_deref())
        .collect()
}

async fn running_ids(rows: &[IntentRow], docker: &Docker) -> Result<Vec<String>, EnsureError> {
    let mut running = Vec::new();
    for row in rows {
        let Some(id) = row.docker_id.as_deref().filter(|id| !id.is_empty()) else {
            continue;
        };
        if container_running(docker, id).await? {
            running.push(id.to_owned());
        }
    }
    Ok(running)
}

fn map_journal(error: HostError) -> EnsureError {
    match error {
        HostError::Endpoint => EnsureError::Endpoint,
        _ => EnsureError::Unexpected {
            status: 0,
            step: "journal",
        },
    }
}

#[cfg(test)]
mod tests;
