// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};
use zeroize::Zeroizing;

use crate::config::{HostConfig, RepositoryConnection};
use crate::error::HostError;
use crate::host_controller::connection_pat;
use crate::journal::Journal;
use crate::reconcile::{IntentRow, Reconcile, before_advertise, occupies};
use crate::scale_set::{EnsureError, observe_scale_set_runners};
use bollard::Docker;

use super::inspect::{ContainerObservation, observe_container};

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

/// The gate decision plus one operator-facing line per blocking row.
pub(crate) struct GateReport {
    pub(crate) decision: Reconcile,
    pub(crate) hold_lines: Vec<String>,
}

/// What the GitHub half of the gate saw. `unobserved` names, per scale set, why no listing was
/// obtained, so a Hold is never anonymous.
pub(crate) struct GithubObservation {
    runners: Result<Vec<String>, EnsureError>,
    unobserved: BTreeMap<i64, &'static str>,
}

impl GithubObservation {
    pub(crate) fn listed(runners: Result<Vec<String>, EnsureError>) -> Self {
        Self {
            runners,
            unobserved: BTreeMap::new(),
        }
    }

    fn failed(reason: &'static str, scale_sets: &BTreeSet<i64>) -> Self {
        Self {
            runners: Err(EnsureError::Uncertain),
            unobserved: scale_sets.iter().map(|id| (*id, reason)).collect(),
        }
    }
}

pub(crate) async fn evaluate_gate(
    journal: &Journal,
    docker: &Docker,
    config: &HostConfig,
) -> Result<GateReport, EnsureError> {
    let rows = journal.rows().await.map_err(map_journal)?;
    let config = config.clone();
    evaluate_rows(rows, docker, move |rows| observe_github(rows, &config)).await
}

/// Runs the blocking GitHub half (HTTPS and the keychain read) off the async runtime, so a slow
/// GitHub cannot stall the controller loop that drives the 30 s retry.
async fn evaluate_rows<F>(
    rows: Vec<IntentRow>,
    docker: &Docker,
    github: F,
) -> Result<GateReport, EnsureError>
where
    F: FnOnce(&[IntentRow]) -> GithubObservation + Send + 'static,
{
    let (observed_docker, absent_docker) = running_ids(&rows, docker).await?;
    let for_github = rows.clone();
    let github = tokio::task::spawn_blocking(move || github(&for_github))
        .await
        .unwrap_or_else(|_| {
            GithubObservation::failed("GitHub observation aborted", &all_sets(&rows))
        });
    let hold_lines = hold::hold_lines(&rows, &observed_docker, &absent_docker, &github);
    let decision = decide(&rows, &observed_docker, github.runners);
    Ok(GateReport {
        decision,
        hold_lines,
    })
}

fn all_sets(rows: &[IntentRow]) -> BTreeSet<i64> {
    rows.iter().filter_map(|row| row.scale_set_id).collect()
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

type ListRunners = fn(&str, &RepositoryConnection, i64) -> Result<Option<Vec<String>>, EnsureError>;

/// Where the gate's GitHub half gets credentials and listings. Tests swap in stubs.
struct GithubSources {
    credential: fn(&RepositoryConnection) -> Result<Zeroizing<Vec<u8>>, HostError>,
    list: ListRunners,
}

const LIVE: GithubSources = GithubSources {
    credential: |binding| binding.credential_ref.load(),
    list: observe_scale_set_runners,
};

fn observe_github(rows: &[IntentRow], config: &HostConfig) -> GithubObservation {
    observe_with(rows, config, &LIVE)
}

fn observe_with(
    rows: &[IntentRow],
    config: &HostConfig,
    sources: &GithubSources,
) -> GithubObservation {
    let mut pending = match scale_sets_to_observe(rows) {
        Ok(pending) => pending,
        Err(error) => {
            return GithubObservation {
                runners: Err(error),
                unobserved: BTreeMap::new(),
            };
        }
    };
    let mut observed = Vec::new();
    let mut credential_gap = None;
    for binding in config.connections() {
        if pending.is_empty() {
            break;
        }
        let Ok(credential) = (sources.credential)(binding) else {
            credential_gap = Some("keychain secret missing");
            continue;
        };
        let Ok(pat) = connection_pat(&credential) else {
            credential_gap = Some("connection credential unusable");
            continue;
        };
        for scale_set_id in pending.clone() {
            match (sources.list)(pat, binding, scale_set_id) {
                Ok(Some(runners)) => {
                    observed.extend(runners);
                    pending.remove(&scale_set_id);
                }
                Ok(None) => {}
                Err(error) => {
                    return GithubObservation {
                        runners: Err(error),
                        unobserved: pending
                            .iter()
                            .map(|id| (*id, "GitHub listing failed"))
                            .collect(),
                    };
                }
            }
        }
    }
    if pending.is_empty() {
        return GithubObservation::listed(Ok(observed));
    }
    // No credential gap and no listing error: every current connection answered that it does not
    // own the set, so its connection is no longer in the config.
    let reason = credential_gap.unwrap_or("connection removed");
    GithubObservation::failed(reason, &pending)
}

fn str_refs(ids: &[String]) -> Vec<&str> {
    ids.iter().map(String::as_str).collect()
}

fn owned_ids(rows: &[IntentRow]) -> Vec<&str> {
    rows.iter()
        .filter_map(|row| row.docker_id.as_deref())
        .collect()
}

async fn running_ids(
    rows: &[IntentRow],
    docker: &Docker,
) -> Result<(Vec<String>, Vec<String>), EnsureError> {
    let mut running = Vec::new();
    let mut absent = Vec::new();
    for row in rows {
        let Some(id) = row.docker_id.as_deref().filter(|id| !id.is_empty()) else {
            continue;
        };
        match observe_container(docker, id).await? {
            ContainerObservation::Live => running.push(id.to_owned()),
            ContainerObservation::Absent => absent.push(id.to_owned()),
            ContainerObservation::Stopped => {}
        }
    }
    Ok((running, absent))
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

mod hold;
#[cfg(test)]
mod removal_tests;
#[cfg(test)]
mod tests;
