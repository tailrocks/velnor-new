//! Exact launch discovery for daemon restart and ambiguous Docker responses.

use std::collections::HashMap;

use crate::error::HostError;
use crate::launch_identity::LaunchIdentity;
use crate::worker::{ResourceBudget, container_labels, container_name, identity_labels_match};

use super::pair::PairEngine;

/// Exact containers found for one durable launch.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct ObservedWorker {
    dind_id: Option<String>,
    runner_id: Option<String>,
    runner_running: Option<bool>,
}

impl ObservedWorker {
    /// `DinD` ID found by exact launch labels, if it exists.
    #[must_use]
    pub(crate) fn dind_id(&self) -> Option<&str> {
        self.dind_id.as_deref()
    }

    /// Runner ID found by exact launch labels, if it exists.
    #[must_use]
    pub(crate) fn runner_id(&self) -> Option<&str> {
        self.runner_id.as_deref()
    }

    /// Running state from the exact-label-verified runner inspect, if present.
    #[must_use]
    pub(crate) const fn runner_running(&self) -> Option<bool> {
        self.runner_running
    }
}

/// Discover and verify resources for one launch after restart or an uncertain response.
///
/// The caller persists any discovered ID before it retries a later launch step.
/// This function never treats a failed Docker request as absence.
pub(crate) async fn reconcile_worker<E: PairEngine>(
    engine: &E,
    identity: &LaunchIdentity,
    recorded_runner_id: Option<&str>,
    recorded_dind_id: Option<&str>,
) -> Result<ObservedWorker, HostError> {
    reconcile_worker_inner(engine, identity, recorded_runner_id, recorded_dind_id, None).await
}

pub(crate) async fn reconcile_worker_with_budget<E: PairEngine>(
    engine: &E,
    identity: &LaunchIdentity,
    recorded_runner_id: Option<&str>,
    recorded_dind_id: Option<&str>,
    resource_budget: ResourceBudget,
) -> Result<ObservedWorker, HostError> {
    reconcile_worker_inner(
        engine,
        identity,
        recorded_runner_id,
        recorded_dind_id,
        Some(resource_budget),
    )
    .await
}

async fn reconcile_worker_inner<E: PairEngine>(
    engine: &E,
    identity: &LaunchIdentity,
    recorded_runner_id: Option<&str>,
    recorded_dind_id: Option<&str>,
    resource_budget: Option<ResourceBudget>,
) -> Result<ObservedWorker, HostError> {
    engine.verify_engine(identity).await?;
    let rows = engine.list_launch(identity).await?;
    let mut observed = ObservedWorker::default();
    for row in rows {
        let role = row
            .labels
            .get("velnor.role")
            .map(String::as_str)
            .ok_or(HostError::Ownership)?;
        if !identity_labels_match(&expected_labels(identity, role), &row.labels) {
            return Err(HostError::Ownership);
        }
        let slot = match role {
            "dind" if observed.dind_id.is_none() => &mut observed.dind_id,
            "runner" if observed.runner_id.is_none() => &mut observed.runner_id,
            _ => return Err(HostError::Ownership),
        };
        *slot = Some(row.id);
    }
    check_recorded_id(engine, recorded_dind_id, observed.dind_id.as_deref()).await?;
    check_recorded_id(engine, recorded_runner_id, observed.runner_id.as_deref()).await?;
    verify_named_identity(engine, identity, "dind", observed.dind_id.as_deref()).await?;
    verify_named_identity(engine, identity, "runner", observed.runner_id.as_deref()).await?;
    if let Some(dind_id) = observed.dind_id.as_deref() {
        verify_container(engine, identity, "dind", dind_id, None, resource_budget).await?;
    }
    if let Some(runner_id) = observed.runner_id.as_deref() {
        let dind_id = observed.dind_id.as_deref().ok_or(HostError::Ownership)?;
        let runner = verify_container(
            engine,
            identity,
            "runner",
            runner_id,
            Some(dind_id),
            resource_budget,
        )
        .await?;
        observed.runner_running = runner.running;
    }
    Ok(observed)
}

async fn verify_container<E: PairEngine>(
    engine: &E,
    identity: &LaunchIdentity,
    role: &str,
    id: &str,
    dind_id: Option<&str>,
    resource_budget: Option<ResourceBudget>,
) -> Result<super::ContainerRecord, HostError> {
    match resource_budget {
        Some(budget) => {
            engine
                .verify_container_with_budget(identity, role, id, dind_id, budget, false)
                .await
        }
        None => {
            engine
                .verify_container(identity, role, id, dind_id, false)
                .await
        }
    }
}

async fn check_recorded_id<E: PairEngine>(
    engine: &E,
    recorded: Option<&str>,
    observed: Option<&str>,
) -> Result<(), HostError> {
    match (recorded, observed) {
        (Some(recorded), Some(observed)) if recorded == observed => Ok(()),
        (Some(_), Some(_)) => Err(HostError::Ownership),
        (Some(recorded), None) => match engine.inspect_container(recorded).await? {
            None => Ok(()),
            Some(_) => Err(HostError::Ownership),
        },
        (None, _) => Ok(()),
    }
}

async fn verify_named_identity<E: PairEngine>(
    engine: &E,
    identity: &LaunchIdentity,
    role: &str,
    observed_id: Option<&str>,
) -> Result<(), HostError> {
    let named = engine
        .inspect_container(&container_name(identity, role))
        .await?;
    match (named, observed_id) {
        (None, None) => Ok(()),
        (Some(named), Some(id)) if named.id == id => Ok(()),
        _ => Err(HostError::Ownership),
    }
}

fn expected_labels(identity: &LaunchIdentity, role: &str) -> HashMap<String, String> {
    container_labels(identity, role)
        .iter()
        .filter_map(|label| label.split_once('='))
        .map(|(key, value)| (key.to_owned(), value.to_owned()))
        .collect()
}
