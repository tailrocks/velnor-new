//! Journal acceptance of proof-backed physical worker cleanup.

use crate::error::HostError;

use super::Journal;

/// Post-action evidence kept separate from physical worker cleanup.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PostActionDisposition {
    /// The provider completed all job post-actions successfully.
    Completed,
    /// No job ran on this worker generation, so no post-action was expected.
    NotRun,
    /// An interruption was observed while post-actions were running.
    Interrupted {
        /// Bounded reason class retained for non-success post-actions.
        reason_class: String,
    },
    /// The remote post-action result is not known.
    Unknown,
}

/// Whether physical cleanup completed normally or during an interruption.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CleanupDisposition {
    /// All owned local resources were removed after graceful completion.
    Completed,
    /// All owned local resources were removed after an evidenced interruption.
    Interrupted {
        /// Bounded reason class retained for interrupted cleanup.
        reason_class: String,
    },
}

/// Validated host-side evidence for one exact worker generation.
///
/// The host implementation uses a private constructor and returns this trait
/// only after its durable generation fence, log-retention checks, and final
/// resource absence checks. This is an application trust boundary inside the
/// trusted control plane, not an authorization boundary against arbitrary code.
pub trait PhysicalCleanupProof {
    /// Durable journal generation id.
    fn launch_id(&self) -> i64;
    /// JIT runner name requested by this generation.
    fn expected_runner_name(&self) -> &str;
    /// Durable worker-volume identity.
    fn worker_volume(&self) -> &str;
    /// Exact outer runner container id.
    fn runner_container_id(&self) -> &str;
    /// Exact private `DinD` container id.
    fn dind_container_id(&self) -> &str;
    /// Optional private outer network name.
    fn outer_network_name(&self) -> Option<&str>;
    /// Optional private outer network id.
    fn outer_network_id(&self) -> Option<&str>;
    /// Whether the exact private outer network is absent after cleanup.
    fn outer_network_absent(&self) -> bool;
    /// Workflow run id observed from the actual runner lifecycle event.
    fn observed_workflow_run_id(&self) -> Option<i64>;
    /// Exact Actions REST attempt, when reconciled from the observed event.
    fn observed_attempt(&self) -> Option<i64>;
    /// Opaque Scale Set job id observed from the actual runner event.
    fn observed_job_id(&self) -> Option<&str>;
    /// Numeric Actions REST job id, kept distinct from the Scale Set id.
    fn observed_actions_job_id(&self) -> Option<i64>;
    /// Actual GitHub runner id observed from the actual runner event.
    fn observed_runner_id(&self) -> Option<i64>;
    /// Actual GitHub runner name observed from the actual runner event.
    fn observed_runner_name(&self) -> Option<&str>;
    /// Whether all registration/start side effects for this generation are fenced.
    fn launch_fenced(&self) -> bool;
    /// Persisted exact host observation of the runner start state.
    fn runner_start_observation(&self) -> RunnerStartObservation;
    /// Whether exact runner, `DinD`, children, networks, and volumes are absent.
    fn all_owned_children_networks_and_volumes_absent(&self) -> bool;
    /// Relative path of the retained redacted diagnostics archive.
    fn diagnostics_relative_path(&self) -> &str;
    /// SHA-256 digest of the retained diagnostics archive.
    fn diagnostics_sha256(&self) -> &str;
    /// Retained archive size in bytes.
    fn diagnostics_bytes(&self) -> u64;
    /// Whether diagnostics were redacted before they were retained.
    fn diagnostics_redacted(&self) -> bool;
    /// Whether diagnostics are retained on the protected host path.
    fn diagnostics_retained(&self) -> bool;
    /// Whether no diagnostics source existed because the workload never ran.
    fn diagnostics_source_absent(&self) -> bool;
    /// Exact nested container ids proved absent.
    fn removed_child_container_ids(&self) -> &[String];
    /// Exact nested network ids proved absent.
    fn removed_child_network_ids(&self) -> &[String];
    /// Exact owned named volumes proved absent.
    fn absent_volume_names(&self) -> &[String];
    /// Post-action evidence for this physical generation.
    fn post_actions(&self) -> PostActionDisposition;
    /// Physical cleanup result.
    fn cleanup_disposition(&self) -> CleanupDisposition;
}

/// Host evidence that one exact provisioning-only outer network was removed
/// or that an uncertain deterministic-name create was settled absent.
///
/// This proves only that named local network is absent. It never resolves
/// `AcquireJobs`/JIT uncertainty or releases the launch permit.
pub trait OuterNetworkRemovalProof {
    /// Journal launch generation id.
    fn launch_id(&self) -> i64;
    /// Deterministic network name committed before Docker create.
    fn network_name(&self) -> &str;
    /// Exact network id durably bound after reconciliation/create.
    fn network_id(&self) -> Option<&str>;
    /// Exact Velnor owner/role/launch labels were verified for a found object.
    fn exact_owned_labels_verified(&self) -> bool;
    /// The timed-out create is known to be settled, not merely absent at one instant.
    fn create_effect_resolved(&self) -> bool;
    /// Inspect after removal proved this exact network absent.
    fn network_absent(&self) -> bool;
}

struct CleanupRecord {
    launch_id: i64,
    expected_runner_name: String,
    worker_volume: String,
    runner_id: String,
    dind_id: String,
    outer_network_name: Option<String>,
    outer_network_id: Option<String>,
    observed_workflow_run_id: Option<i64>,
    observed_attempt: Option<i64>,
    observed_job_id: Option<String>,
    observed_actions_job_id: Option<i64>,
    observed_runner_id: Option<i64>,
    observed_runner_name: Option<String>,
    diagnostics_path: String,
    diagnostics_sha256: String,
    diagnostics_bytes: i64,
    diagnostics_source_absent: bool,
    runner_start_observation: RunnerStartObservation,
    post_action_state: &'static str,
    post_action_reason: Option<String>,
    cleanup_state: &'static str,
    cleanup_reason: Option<String>,
    cleanup_resources: String,
}

mod checkpoint;
mod persist;
mod validation;

pub use checkpoint::{
    CleanupCheckpointIdentity, CleanupChildren, CleanupDiagnostics, CleanupStopPolicy,
    OuterNetworkCleanupState, RunnerStartObservation,
};

use persist::persist_cleanup;

impl Journal {
    /// Persist a host-issued physical cleanup proof for one known generation.
    ///
    /// This cannot resolve an unknown `AcquireJobs` effect. It also does not
    /// mark the remote job successful; interrupted outcomes remain history.
    ///
    /// # Errors
    ///
    /// Returns `HostError::Journal` when the proof is incomplete, mismatched,
    /// associated with an unresolved acquire, or cannot be committed.
    pub async fn record_physical_cleanup<P: PhysicalCleanupProof>(
        &self,
        proof: &P,
    ) -> Result<(), HostError> {
        let record = validation::from_proof(proof)?;
        let conn = self.connection().await?;
        conn.execute("BEGIN IMMEDIATE", ())
            .await
            .map_err(|_| HostError::Journal)?;
        let result = persist_cleanup(&conn, &record).await;
        let ended = if result.is_ok() {
            conn.execute("COMMIT", ()).await
        } else {
            conn.execute("ROLLBACK", ()).await
        };
        ended.map_err(|_| HostError::Journal)?;
        result
    }
}
