/// Locally proven termination and resource-cleanup receipt.
///
/// This proof says nothing about the GitHub job result. A caller may release
/// physical capacity when this proof exists, while retaining remote result as
/// unknown. Job success is true only for `Completed` post-actions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkerTerminationProof {
    identity: WorkerGenerationIdentity,
    outer_network_absent: bool,
    runner_start_observation: velnor_runner_journal::journal::RunnerStartObservation,
    post_actions: PostActionDisposition,
    diagnostics: DiagnosticsReceipt,
    children: ChildCleanupEvidence,
    absent_containers: Vec<String>,
    absent_volumes: Vec<String>,
    runner_forced: bool,
}

/// Physical cleanup outcome, separate from the GitHub job conclusion.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CleanupDisposition {
    /// The generation was not force-stopped and its local post-actions are terminal.
    Completed,
    /// Physical resources were reclaimed without a successful local outcome.
    Interrupted {
        /// Bounded outcome class persisted for a non-success cleanup.
        reason_class: &'static str,
    },
}

impl WorkerTerminationProof {
    /// Exact generation covered by the host proof.
    #[must_use]
    pub fn identity(&self) -> &WorkerGenerationIdentity {
        &self.identity
    }

    /// Durable launch id covered by the cleanup fence.
    #[must_use]
    pub const fn launch_id(&self) -> i64 {
        self.identity.launch_id()
    }

    /// Expected runner name recorded before provisioning.
    #[must_use]
    pub fn expected_runner_name(&self) -> &str {
        self.identity.expected_runner_name()
    }

    /// Exact worker volume identity recorded before provisioning.
    #[must_use]
    pub fn worker_volume(&self) -> &str {
        self.identity.worker_volume()
    }

    /// Exact recorded outer runner container id.
    #[must_use]
    pub fn runner_container_id(&self) -> &str {
        self.identity.runner_container_id()
    }

    /// Exact recorded private `DinD` container id.
    #[must_use]
    pub fn dind_container_id(&self) -> &str {
        self.identity.dind_container_id()
    }

    /// Exact optional outer bridge identity covered by cleanup.
    #[must_use]
    pub fn outer_network(&self) -> Option<(&str, &str)> {
        Some((
            self.identity.outer_network_name()?,
            self.identity.outer_network_id()?,
        ))
    }

    /// Whether the exact optional outer bridge is absent after cleanup.
    #[must_use]
    pub const fn outer_network_absent(&self) -> bool {
        self.outer_network_absent
    }

    /// Durable exact start observation used to choose post-action disposition.
    #[must_use]
    pub const fn runner_start_observation(
        &self,
    ) -> velnor_runner_journal::journal::RunnerStartObservation {
        self.runner_start_observation
    }

    /// Actual observed workflow/job/runner identities, if lifecycle evidence exists.
    #[must_use]
    pub fn observed_job(&self) -> Option<&ObservedJobIdentity> {
        self.identity.observed_job()
    }

    /// Workflow run id from an observed Scale Set event, when available.
    #[must_use]
    pub fn workflow_run_id(&self) -> Option<u64> {
        self.observed_job()
            .map(ObservedJobIdentity::workflow_run_id)
    }

    /// Authoritative workflow attempt, when REST reconciliation supplied it.
    #[must_use]
    pub fn attempt(&self) -> Option<u32> {
        self.observed_job().and_then(ObservedJobIdentity::attempt)
    }

    /// Opaque Scale Set job id from the lifecycle event, when available.
    #[must_use]
    pub fn scale_set_job_id(&self) -> Option<&str> {
        self.observed_job()
            .map(ObservedJobIdentity::scale_set_job_id)
    }

    /// Numeric Actions job id from REST reconciliation, when available.
    #[must_use]
    pub fn actions_job_id(&self) -> Option<u64> {
        self.observed_job()
            .and_then(ObservedJobIdentity::actions_job_id)
    }

    /// Actual GitHub runner id from the lifecycle event, when available.
    #[must_use]
    pub fn observed_runner_id(&self) -> Option<u64> {
        self.observed_job().map(ObservedJobIdentity::runner_id)
    }

    /// Actual GitHub runner name from the lifecycle event, when available.
    #[must_use]
    pub fn observed_runner_name(&self) -> Option<&str> {
        self.observed_job().map(ObservedJobIdentity::runner_name)
    }

    /// The durable generation lease was accepted before any Docker effect.
    #[must_use]
    pub const fn launch_fenced(&self) -> bool {
        // This type has no public constructor; the cleanup flow creates it
        // only after the durable `begin` fence succeeds.
        true
    }

    /// Post-action disposition preserved without inferring remote success.
    #[must_use]
    pub fn post_actions(&self) -> &PostActionDisposition {
        &self.post_actions
    }

    /// Durable redacted runner diagnostic receipt.
    #[must_use]
    pub fn diagnostics(&self) -> &DiagnosticsReceipt {
        &self.diagnostics
    }

    /// Private relative path of the retained redacted diagnostics archive.
    #[must_use]
    pub fn diagnostics_relative_path(&self) -> &str {
        self.diagnostics.relative_path()
    }

    /// SHA-256 of the retained redacted diagnostics archive.
    #[must_use]
    pub fn diagnostics_sha256(&self) -> &str {
        self.diagnostics.sha256()
    }

    /// Bytes in the retained diagnostics archive.
    #[must_use]
    pub const fn diagnostics_bytes(&self) -> u64 {
        self.diagnostics.bytes()
    }

    /// Whether the diagnostics archive was redacted before retention.
    #[must_use]
    pub const fn diagnostics_redacted(&self) -> bool {
        self.diagnostics.redacted()
    }

    /// Whether the diagnostics archive is retained under the host-controlled root.
    #[must_use]
    pub const fn diagnostics_retained(&self) -> bool {
        self.diagnostics.retained()
    }

    /// Whether the runner diagnostic source was absent because work never ran.
    #[must_use]
    pub const fn diagnostics_source_absent(&self) -> bool {
        self.diagnostics.source_absent()
    }

    /// Child Docker resources observed and removed from private `DinD`.
    #[must_use]
    pub fn children(&self) -> &ChildCleanupEvidence {
        &self.children
    }

    /// Whether the exact private daemon was observed stopped before removal.
    #[must_use]
    pub const fn dind_stopped(&self) -> bool {
        // Construction occurs only after `stop_dind_after_children` returns
        // exact stopped-state evidence.
        true
    }

    /// Exact outer container ids now absent.
    #[must_use]
    pub fn absent_containers(&self) -> &[String] {
        &self.absent_containers
    }

    /// Exact named volumes now absent.
    #[must_use]
    pub fn absent_volumes(&self) -> &[String] {
        &self.absent_volumes
    }

    /// All recorded inner children, outer containers, and worker volumes are absent.
    #[must_use]
    pub fn all_owned_children_networks_and_volumes_absent(&self) -> bool {
        if !self.outer_network_absent
            || !self.dind_stopped()
            || self.absent_containers.len() != 2
            || !self
                .absent_containers
                .iter()
                .any(|id| id == self.identity.runner_container_id())
            || !self
                .absent_containers
                .iter()
                .any(|id| id == self.identity.dind_container_id())
        {
            return false;
        }
        let Ok(mut expected_volumes) =
            crate::worker::volumes::worker_volume_names(self.identity.worker_volume())
        else {
            return false;
        };
        let mut absent_volumes = self.absent_volumes.clone();
        expected_volumes.sort_unstable();
        absent_volumes.sort_unstable();
        absent_volumes == expected_volumes
    }

    /// True when Docker had to kill the runner after the configured grace.
    #[must_use]
    pub const fn runner_forced(&self) -> bool {
        self.runner_forced
    }

    /// Whether the local post-action sequence completed successfully.
    #[must_use]
    pub const fn post_actions_completed(&self) -> bool {
        matches!(self.post_actions, PostActionDisposition::Completed)
    }

    /// Local physical cleanup result; this never asserts remote job success.
    #[must_use]
    pub const fn cleanup_disposition(&self) -> CleanupDisposition {
        if self.runner_forced {
            return CleanupDisposition::Interrupted {
                reason_class: "forced_stop",
            };
        }
        match &self.post_actions {
            PostActionDisposition::Completed | PostActionDisposition::NotRun => {
                CleanupDisposition::Completed
            }
            PostActionDisposition::Interrupted { .. } => CleanupDisposition::Interrupted {
                reason_class: "post_actions_interrupted",
            },
            PostActionDisposition::Unknown => CleanupDisposition::Interrupted {
                reason_class: "unknown_post_actions",
            },
        }
    }

    /// Physical capacity may be reclaimed only after this sealed proof exists.
    #[must_use]
    pub const fn physical_capacity_reclaimable(&self) -> bool {
        true
    }
}

/// Durable journal boundary used by host cleanup; implementations must commit
/// each intent/checkpoint before returning success and hold one exclusive
/// cleanup lease for this exact generation until completion or release.
#[expect(
    async_fn_in_trait,
    reason = "workspace uses native async traits for injectable effect boundaries"
)]
pub trait CleanupLedger {
    /// Persist a generation-fenced intent before any Docker mutation.
    ///
    /// Reject unresolved acquire/registration/JIT effects. `NotRun` requires
    /// durable evidence that the workload was never started.
    async fn begin(
        &self,
        identity: &WorkerGenerationIdentity,
        post_actions: &PostActionDisposition,
        stop_policy: &RunnerStopPolicy,
    ) -> Result<(), HostError>;

    /// Read durable pre-start intent or an earlier cleanup observation.
    async fn runner_start_observation(
        &self,
        identity: &WorkerGenerationIdentity,
    ) -> Result<Option<velnor_runner_journal::journal::RunnerStartObservation>, HostError>;

    /// Persist an exact inspected start state after the cleanup fence.
    async fn record_runner_start_observation(
        &self,
        identity: &WorkerGenerationIdentity,
        observation: velnor_runner_journal::journal::RunnerStartObservation,
    ) -> Result<(), HostError>;

    /// Persist intent before the named external operation.
    async fn before(&self, launch_id: i64, step: &CleanupStep) -> Result<(), HostError>;

    /// Persist verified result for one completed operation.
    async fn after(&self, launch_id: i64, step: &CleanupStep) -> Result<(), HostError>;

    /// Merge the exact child ids observed before child-removal requests.
    async fn observe_children(
        &self,
        identity: &WorkerGenerationIdentity,
        inventory: &ChildResourceInventory,
    ) -> Result<(), HostError>;

    /// Load the accumulated ids observed for this exact cleanup generation.
    async fn observed_children(
        &self,
        identity: &WorkerGenerationIdentity,
    ) -> Result<ChildCleanupEvidence, HostError>;

    /// Load proof that a final private-daemon query returned empty.
    async fn prior_children_drained(
        &self,
        identity: &WorkerGenerationIdentity,
    ) -> Result<Option<ChildCleanupEvidence>, HostError>;

    /// Persist an empty final query with the exact previously observed child ids.
    async fn children_drained(
        &self,
        identity: &WorkerGenerationIdentity,
        evidence: &ChildCleanupEvidence,
    ) -> Result<(), HostError>;

    /// Persist a redacted log receipt before any runner/home-volume deletion.
    async fn diagnostics(
        &self,
        launch_id: i64,
        receipt: &DiagnosticsReceipt,
    ) -> Result<(), HostError>;

    /// Persist the sealed physical proof after all resource absence checks.
    async fn complete(&self, proof: &WorkerTerminationProof) -> Result<(), HostError>;
}
