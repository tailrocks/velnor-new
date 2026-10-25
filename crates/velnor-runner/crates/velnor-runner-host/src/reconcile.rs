use crate::journal::IntentState;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LaunchPhase {
    Prepared,

    AcquireRequested,

    Acquired,

    JitRequested,

    JitReceived,

    DockerProvisioning,

    WorkerReady,

    AcknowledgementRequested,

    Complete,
}

impl LaunchPhase {
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
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "DB row mirror: each bool is an independent journal column"
)]
pub struct IntentRow {
    pub id: i64,

    pub kind: String,

    pub subject: String,

    pub state: IntentState,

    pub docker_id: Option<String>,

    pub dind_id: Option<String>,

    pub worker_volume: Option<String>,

    pub scale_set_id: Option<i64>,

    pub request_id: Option<i64>,

    pub runner_name: Option<String>,

    pub docker_engine_id: Option<String>,

    pub launch_phase: Option<LaunchPhase>,

    pub github_runner_id: Option<String>,

    pub cleanup_proven: bool,

    pub launch_id: Option<String>,

    pub assignment_key: Option<String>,

    pub seed_generation_id: Option<String>,

    pub acquire_attempted: bool,

    pub acquire_resolved: bool,

    pub acquired: bool,

    pub jit_requested: bool,

    pub runner_completed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Reconcile {
    Hold { adopt: Vec<String>, occupied: usize },

    Advertise { occupied: usize },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReleaseFact {
    Pending,

    Uncertain,

    ProvenCleanup,

    DefiniteFailure,
}

#[must_use]
pub const fn release_permitted(outcome: ReleaseFact) -> bool {
    matches!(outcome, ReleaseFact::ProvenCleanup)
}

#[must_use]
pub fn occupies(row: &IntentRow) -> bool {
    if row.cleanup_proven {
        return false;
    }
    !(row.state == IntentState::Failed && row.kind == "acquire")
}

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
