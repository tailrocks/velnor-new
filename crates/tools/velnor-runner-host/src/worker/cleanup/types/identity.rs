/// GitHub job identity observed for this runner generation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObservedJobIdentity {
    workflow_run_id: u64,
    attempt: Option<u32>,
    scale_set_job_id: String,
    actions_job_id: Option<u64>,
    runner_id: u64,
    runner_name: String,
}

impl ObservedJobIdentity {
    /// Validate and preserve all observed GitHub identifiers separately.
    ///
    /// # Errors
    ///
    /// Returns [`HostError::Identity`] when an observed identifier is invalid.
    pub fn new(
        workflow_run_id: u64,
        attempt: Option<u32>,
        scale_set_job_id: String,
        actions_job_id: Option<u64>,
        runner_id: u64,
        runner_name: String,
    ) -> Result<Self, HostError> {
        if workflow_run_id == 0
            || workflow_run_id > i64::MAX as u64
            || attempt.is_some_and(|attempt| attempt == 0)
            || attempt.is_some() != actions_job_id.is_some()
            || scale_set_job_id.is_empty()
            || scale_set_job_id.len() > 256
            || scale_set_job_id.chars().any(char::is_control)
            || actions_job_id.is_some_and(|job_id| job_id == 0)
            || actions_job_id.is_some_and(|job_id| job_id > i64::MAX as u64)
            || runner_id == 0
            || runner_id > i64::MAX as u64
            || !valid_runner_name(&runner_name)
        {
            return Err(HostError::Identity);
        }
        Ok(Self {
            workflow_run_id,
            attempt,
            scale_set_job_id,
            actions_job_id,
            runner_id,
            runner_name,
        })
    }

    /// Workflow run id reported for the actual runner job.
    #[must_use]
    pub const fn workflow_run_id(&self) -> u64 {
        self.workflow_run_id
    }

    /// Attempt from an authoritative Actions job row, when available.
    #[must_use]
    pub const fn attempt(&self) -> Option<u32> {
        self.attempt
    }

    /// Opaque Scale Set job id reported by the lifecycle event.
    #[must_use]
    pub fn scale_set_job_id(&self) -> &str {
        &self.scale_set_job_id
    }

    /// Numeric Actions job id from REST reconciliation, when available.
    #[must_use]
    pub const fn actions_job_id(&self) -> Option<u64> {
        self.actions_job_id
    }

    /// GitHub runner id reported for the actual runner job.
    #[must_use]
    pub const fn runner_id(&self) -> u64 {
        self.runner_id
    }

    /// GitHub runner name reported for the actual runner job.
    #[must_use]
    pub fn runner_name(&self) -> &str {
        &self.runner_name
    }
}

/// Persisted identity for one fenced local worker generation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkerGenerationIdentity {
    launch_id: i64,
    expected_runner_name: String,
    worker_volume: String,
    runner_container_id: String,
    dind_container_id: String,
    outer_network_name: Option<String>,
    outer_network_id: Option<String>,
    observed_job: Option<ObservedJobIdentity>,
}

impl WorkerGenerationIdentity {
    /// Construct an identity from a known, durable launch generation.
    ///
    /// # Errors
    ///
    /// Returns [`HostError::Identity`] when the generation identity is invalid.
    pub fn new(
        launch_id: i64,
        expected_runner_name: String,
        worker_volume: String,
        runner_container_id: String,
        dind_container_id: String,
        observed_job: Option<ObservedJobIdentity>,
    ) -> Result<Self, HostError> {
        Self::new_with_network(
            launch_id,
            expected_runner_name,
            worker_volume,
            runner_container_id,
            dind_container_id,
            None,
            None,
            observed_job,
        )
    }

    /// Construct an identity with an optional exact per-generation outer bridge.
    ///
    /// The bridge name is derived from `worker_volume`; callers cannot attach
    /// cleanup proof to an arbitrary Docker network.
    ///
    /// # Errors
    ///
    /// Returns [`HostError::Identity`] when the generation or network identity
    /// is invalid, incomplete, or differs from the deterministic bridge name.
    #[expect(
        clippy::too_many_arguments,
        reason = "each argument is a separate durable identity field validated together"
    )]
    pub fn new_with_network(
        launch_id: i64,
        expected_runner_name: String,
        worker_volume: String,
        runner_container_id: String,
        dind_container_id: String,
        outer_network_name: Option<String>,
        outer_network_id: Option<String>,
        observed_job: Option<ObservedJobIdentity>,
    ) -> Result<Self, HostError> {
        let expected_network_name = format!("{worker_volume}-outer");
        let network_valid = match (&outer_network_name, &outer_network_id) {
            (None, None) => true,
            (Some(name), Some(id)) => name == &expected_network_name && valid_container_id(id),
            _ => false,
        };
        if launch_id <= 0
            || !valid_runner_name(&expected_runner_name)
            || !valid_worker_name(&worker_volume)
            || !valid_container_id(&runner_container_id)
            || !valid_container_id(&dind_container_id)
            || runner_container_id == dind_container_id
            || !network_valid
            || observed_job
                .as_ref()
                .is_some_and(|job| job.runner_name() != expected_runner_name)
        {
            return Err(HostError::Identity);
        }
        Ok(Self {
            launch_id,
            expected_runner_name,
            worker_volume,
            runner_container_id,
            dind_container_id,
            outer_network_name,
            outer_network_id,
            observed_job,
        })
    }

    /// Durable launch row identity.
    #[must_use]
    pub const fn launch_id(&self) -> i64 {
        self.launch_id
    }

    /// Expected runner name recorded before provisioning.
    #[must_use]
    pub fn expected_runner_name(&self) -> &str {
        &self.expected_runner_name
    }

    /// Unique worker volume base recorded before Docker creates resources.
    #[must_use]
    pub fn worker_volume(&self) -> &str {
        &self.worker_volume
    }

    /// Exact outer runner container id recorded after Docker create.
    #[must_use]
    pub fn runner_container_id(&self) -> &str {
        &self.runner_container_id
    }

    /// Exact private `DinD` container id recorded after Docker create.
    #[must_use]
    pub fn dind_container_id(&self) -> &str {
        &self.dind_container_id
    }

    /// Deterministic per-generation outer bridge name, when this is a Linux job.
    #[must_use]
    pub fn outer_network_name(&self) -> Option<&str> {
        self.outer_network_name.as_deref()
    }

    /// Exact per-generation outer bridge id, when this is a Linux job.
    #[must_use]
    pub fn outer_network_id(&self) -> Option<&str> {
        self.outer_network_id.as_deref()
    }

    /// Actual GitHub job identity, if a lifecycle/API observation exists.
    #[must_use]
    pub fn observed_job(&self) -> Option<&ObservedJobIdentity> {
        self.observed_job.as_ref()
    }
}
