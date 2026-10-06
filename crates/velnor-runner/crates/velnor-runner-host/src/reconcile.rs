//! Restart reconcile. Pure: no database, Docker, or HTTP.

use crate::journal::IntentState;

/// Last external-effect boundary durably reached by one launch.
///
/// `None` on an intent row is a legacy or incomplete record and must remain
/// unknown; it does not prove that no effect was attempted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LaunchPhase {
    /// Identities are committed; no launch effect has been requested.
    Prepared,
    /// Acquire was committed before the request was sent.
    AcquireRequested,
    /// The acquire response confirmed the requested job.
    Acquired,
    /// JIT was committed before the request was sent.
    JitRequested,
    /// JIT returned an encoded configuration, which is never journaled.
    JitReceived,
    /// Docker volume or container provisioning may have begun.
    DockerProvisioning,
    /// The runner pair was created and bound in the journal.
    WorkerReady,
    /// Queue acknowledgement was committed before the request was sent.
    AcknowledgementRequested,
    /// All effects for this launch completed.
    Complete,
}

impl LaunchPhase {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Prepared => "prepared",
            Self::AcquireRequested => "acquire_requested",
            Self::Acquired => "acquired",
            Self::JitRequested => "jit_requested",
            Self::JitReceived => "jit_received",
            Self::DockerProvisioning => "docker_provisioning",
            Self::WorkerReady => "worker_ready",
            Self::AcknowledgementRequested => "acknowledgement_requested",
            Self::Complete => "complete",
        }
    }

    pub(crate) fn parse(text: &str) -> Result<Self, crate::HostError> {
        match text {
            "prepared" => Ok(Self::Prepared),
            "acquire_requested" => Ok(Self::AcquireRequested),
            "acquired" => Ok(Self::Acquired),
            "jit_requested" => Ok(Self::JitRequested),
            "jit_received" => Ok(Self::JitReceived),
            "docker_provisioning" => Ok(Self::DockerProvisioning),
            "worker_ready" => Ok(Self::WorkerReady),
            "acknowledgement_requested" => Ok(Self::AcknowledgementRequested),
            "complete" => Ok(Self::Complete),
            _ => Err(crate::HostError::Journal),
        }
    }

    pub(crate) const fn rank(self) -> u8 {
        match self {
            Self::Prepared => 0,
            Self::AcquireRequested => 1,
            Self::Acquired => 2,
            Self::JitRequested => 3,
            Self::JitReceived => 4,
            Self::DockerProvisioning => 5,
            Self::WorkerReady => 6,
            Self::AcknowledgementRequested => 7,
            Self::Complete => 8,
        }
    }
}

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
    /// Runner container id. Not a name.
    pub docker_id: Option<String>,
    /// Private `DinD` container id. Not a name.
    pub dind_id: Option<String>,
    /// Worker volume base durably recorded before volume creation.
    pub worker_volume: Option<String>,
    /// Scale set used for this launch, when its identity is known.
    pub scale_set_id: Option<i64>,
    /// Acquired request id, when this launch came from an offer.
    pub request_id: Option<i64>,
    /// Exact runner name used for JIT, when known.
    pub runner_name: Option<String>,
    /// Docker engine identity captured before launch effects.
    pub docker_engine_id: Option<String>,
    /// Last durable external-effect boundary; `None` preserves legacy uncertainty.
    pub launch_phase: Option<LaunchPhase>,
    /// GitHub runner id. Not a token.
    pub github_runner_id: Option<String>,
    /// Cleanup of the recorded ids was proven.
    pub cleanup_proven: bool,
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
