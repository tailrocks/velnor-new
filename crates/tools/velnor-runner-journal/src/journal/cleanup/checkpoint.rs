//! Durable checkpoints for exact-generation worker cleanup.

mod network;
mod operations;
mod order;
mod proof;
mod read;
mod start;
mod validation;

pub use network::OuterNetworkCleanupState;

/// Exact host observation of whether this runner generation ever started.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunnerStartObservation {
    /// Inspect proved the exact runner container never entered the started state.
    NeverStarted,
    /// The runner started, may have started, or legacy history cannot prove otherwise.
    MayHaveStarted,
}

impl RunnerStartObservation {
    pub(super) fn as_str(self) -> &'static str {
        match self {
            Self::NeverStarted => "never_started",
            Self::MayHaveStarted => "may_have_started",
        }
    }

    pub(super) fn parse(value: &str) -> Result<Self, crate::HostError> {
        match value {
            "never_started" => Ok(Self::NeverStarted),
            "may_have_started" => Ok(Self::MayHaveStarted),
            _ => Err(crate::HostError::Journal),
        }
    }
}

/// Persisted identities supplied by the host cleanup boundary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CleanupCheckpointIdentity {
    /// Journal launch generation id.
    pub launch_id: i64,
    /// JIT runner name persisted before provisioning.
    pub expected_runner_name: String,
    /// Velnor-owned worker volume base.
    pub worker_volume: String,
    /// Exact runner container id persisted after create.
    pub runner_container_id: String,
    /// Exact private `DinD` container id persisted after create.
    pub dind_container_id: String,
    /// Optional per-generation outer network name.
    pub outer_network_name: Option<String>,
    /// Optional per-generation outer network id.
    pub outer_network_id: Option<String>,
    /// Actual workflow run id observed from a runner event.
    pub observed_workflow_run_id: Option<i64>,
    /// Exact Actions attempt from read-only REST reconciliation.
    pub observed_attempt: Option<i64>,
    /// Opaque Scale Set job id observed from a runner event.
    pub observed_job_id: Option<String>,
    /// Numeric Actions REST job id, distinct from the Scale Set id.
    pub observed_actions_job_id: Option<i64>,
    /// Actual GitHub runner id observed from a runner event.
    pub observed_runner_id: Option<i64>,
    /// Actual GitHub runner name observed from a runner event.
    pub observed_runner_name: Option<String>,
}

/// Stop behavior that was durably selected before cleanup starts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CleanupStopPolicy {
    /// Require the runner to be stopped already.
    RequireStopped,
    /// Stop gracefully, then force-stop after the grace interval.
    StopAtDeadline {
        /// Grace period in seconds before force stop.
        grace_seconds: u32,
        /// Bounded reason class for interruption.
        reason_class: String,
    },
}

/// Exact private Docker child inventory accumulated across cleanup retries.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CleanupChildren {
    /// Child container ids observed in private `DinD`.
    pub containers: Vec<String>,
    /// Custom network ids observed in private `DinD`.
    pub networks: Vec<String>,
}

/// Redacted diagnostic receipt persisted before owned containers are removed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CleanupDiagnostics {
    /// Relative path beneath the protected diagnostics root, or empty for `NotRun`.
    pub relative_path: String,
    /// SHA-256 digest of the retained archive.
    pub sha256: String,
    /// Archive size in bytes.
    pub bytes: u64,
    /// Archive was redacted before retention.
    pub redacted: bool,
    /// Archive and metadata were durably retained.
    pub retained: bool,
    /// No diagnostic source existed because the workload never ran.
    pub source_absent: bool,
}
