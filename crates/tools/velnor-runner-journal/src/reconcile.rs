//! Restart reconcile. Pure: no database, Docker, or HTTP.

use crate::journal::{IntentState, LaunchEffectState, RunnerStartIntent};

/// One durable intent loaded for reconcile.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IntentRow {
    /// Row id.
    pub id: i64,
    /// Effect kind such as `acquire` or `delete`.
    pub kind: String,
    /// Job or request id. Replay key with `kind`.
    pub subject: String,
    /// Durable state.
    pub state: IntentState,
    /// Monotonic evidence about whether an external launch effect may have run.
    pub launch_effect: LaunchEffectState,
    /// Runner container id. Not a name.
    pub docker_id: Option<String>,
    /// Private `DinD` container id. Not a name.
    pub dind_id: Option<String>,
    /// Worker volume base durably recorded before volume creation.
    pub worker_volume: Option<String>,
    /// GitHub runner id. Not a token.
    pub github_runner_id: Option<String>,
    /// Service message id. Distinct from the acquired request id.
    pub message_id: Option<i64>,
    /// Acquired request id. It does not bind the request to a JIT runner.
    pub runner_request_id: Option<i64>,
    /// Workflow run id from the Available request, when supplied.
    pub requested_workflow_run_id: Option<i64>,
    /// Opaque job id from the Available request, when supplied.
    pub requested_job_id: Option<String>,
    /// Exact JIT runner name submitted by this launch generation.
    pub runner_name: Option<String>,
    /// Opaque job id observed from the actual runner lifecycle event.
    pub observed_job_id: Option<String>,
    /// Workflow run id observed from the actual runner lifecycle event.
    pub observed_workflow_run_id: Option<i64>,
    /// A matching `JobCompleted` event was durably observed.
    pub remote_terminal: bool,
    /// Cleanup of the recorded ids was proven.
    pub cleanup_proven: bool,
    /// Deterministic private bridge name durably stored before network creation.
    pub outer_network_name: Option<String>,
    /// Exact Docker network ID returned by host inspection after creation.
    pub outer_network_id: Option<String>,
    /// Durable intent immediately before runner start; migrated rows stay unknown.
    pub runner_start_intent: RunnerStartIntent,
}

/// Whether the host may advertise free capacity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Reconcile {
    /// Keep capacity unpublished. Do not delete.
    Hold {
        /// Owned docker ids that have no journal row.
        adopt: Vec<String>,
        /// Rows that still consume a permit.
        occupied: usize,
    },
    /// Journal and observations agree.
    Advertise {
        /// Rows that still consume a permit.
        occupied: usize,
    },
}

/// Fact offered to [`release_permitted`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReleaseFact {
    /// Intent is still pending.
    Pending,
    /// The effect may have happened.
    Uncertain,
    /// Cleanup proof covers the owned ids.
    ProvenCleanup,
    /// The effect definitely did not happen.
    DefiniteFailure,
}

/// True only for proven cleanup. Pending and uncertain stay occupied.
#[must_use]
pub const fn release_permitted(outcome: ReleaseFact) -> bool {
    matches!(outcome, ReleaseFact::ProvenCleanup)
}

/// Failed `acquire` and proven cleanup do not consume a permit.
#[must_use]
pub fn occupies(row: &IntentRow) -> bool {
    if row.cleanup_proven {
        return false;
    }
    if row.kind == "launch" {
        return !(row.state == IntentState::Failed
            && row.launch_effect == LaunchEffectState::DefiniteNoEffect);
    }
    !(row.state == IntentState::Failed && row.kind == "acquire")
}

/// Decide advertise or hold before publishing free slots.
///
/// Owned docker ids with no journal row are returned for adoption, never deletion.
#[must_use]
pub fn before_advertise(
    rows: &[IntentRow],
    observed_docker: &[&str],
    observed_github: &[&str],
    owned_docker: &[&str],
) -> Reconcile {
    let adopt = adopt_ids(rows, observed_docker, owned_docker);
    let occupied = rows.iter().filter(|row| occupies(row)).count();
    if blocks(rows, observed_docker, observed_github) || !adopt.is_empty() {
        Reconcile::Hold { adopt, occupied }
    } else {
        Reconcile::Advertise { occupied }
    }
}

fn blocks(rows: &[IntentRow], observed_docker: &[&str], observed_github: &[&str]) -> bool {
    rows.iter()
        .any(|row| row_blocks(row, observed_docker, observed_github))
}

fn row_blocks(row: &IntentRow, observed_docker: &[&str], observed_github: &[&str]) -> bool {
    if row.cleanup_proven {
        return false;
    }
    if row.kind == "launch"
        && row.state == IntentState::Failed
        && row.launch_effect != LaunchEffectState::DefiniteNoEffect
    {
        return true;
    }
    if !matches!(row.state, IntentState::Done | IntentState::Failed) {
        return true;
    }
    absent(
        row.docker_id.as_deref(),
        row.cleanup_proven,
        observed_docker,
    ) || absent(
        row.github_runner_id.as_deref(),
        row.cleanup_proven,
        observed_github,
    )
}

fn absent(id: Option<&str>, proven: bool, observed: &[&str]) -> bool {
    match id {
        Some(value) if !proven => !observed.contains(&value),
        _ => false,
    }
}

fn adopt_ids(rows: &[IntentRow], observed: &[&str], owned: &[&str]) -> Vec<String> {
    let mut ids: Vec<&str> = observed
        .iter()
        .copied()
        .filter(|id| owned.contains(id) && !journal_has(rows, id))
        .collect();
    ids.sort_unstable();
    ids.dedup();
    ids.into_iter().map(str::to_owned).collect()
}

fn journal_has(rows: &[IntentRow], id: &str) -> bool {
    rows.iter().any(|row| row.docker_id.as_deref() == Some(id))
}
