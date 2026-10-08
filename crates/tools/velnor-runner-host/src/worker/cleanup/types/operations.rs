use serde::{Deserialize, Serialize};

/// Local post-action disposition, independent of the GitHub job conclusion.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum PostActionDisposition {
    /// The runner's required post-actions completed successfully.
    Completed,
    /// Evidence proves the runner never started the workload.
    NotRun,
    /// The workload or post-actions were stopped for a known reason.
    Interrupted {
        /// Bounded reason class recorded for a non-success physical cleanup.
        reason_class: String,
    },
    /// Post-action disposition could not be established.
    Unknown,
}

/// Explicit policy for a runner that is still live when cleanup begins.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RunnerStopPolicy {
    /// Do not send a signal; an already-stopped runner is required.
    RequireStopped,
    /// Stop gracefully, then kill only after this bounded grace interval.
    StopAtDeadline {
        /// Grace period before force-stopping the owned runner.
        grace_seconds: u32,
        /// Bounded operator or lifecycle reason for forced cleanup.
        reason_class: String,
    },
}

/// Idempotent checkpoint names persisted around external cleanup effects.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CleanupStep {
    /// Signal or verify the exact outer runner container.
    RunnerTermination,
    /// Retrieve and durably retain runner diagnostics.
    DiagnosticsRetention,
    /// Enumerate Docker children owned by the private `DinD` daemon.
    ChildEnumeration,
    /// Persist that a repeated private-daemon query returned no child resources.
    ChildrenDrained,
    /// Stop or verify the exact private `DinD` container after child drain.
    DindTermination,
    /// Remove one exact child container or custom network.
    ChildResourceRemoval {
        /// Exact child container or network ID observed from private `DinD`.
        id: String,
        /// Resource class for the exact child ID.
        kind: ChildResourceKind,
    },
    /// Remove the exact outer runner container.
    RunnerRemoval,
    /// Remove the exact private `DinD` container.
    DindRemoval,
    /// Remove the exact detached outer bridge owned by this Linux generation.
    OuterNetworkRemoval,
    /// Remove the exact Velnor-owned named-volume catalog.
    VolumeRemoval,
}

/// Resource class inside the one-job private `DinD` daemon.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChildResourceKind {
    /// A Docker container created by the job's private daemon.
    Container,
    /// A user-created Docker network created by the job's private daemon.
    Network,
}

/// Evidence returned by the private daemon after all children are absent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChildCleanupEvidence {
    container_ids: Vec<String>,
    network_ids: Vec<String>,
}

impl ChildCleanupEvidence {
    /// The exact inner container ids observed and removed.
    #[must_use]
    pub fn container_ids(&self) -> &[String] {
        &self.container_ids
    }

    /// The exact custom network ids observed and removed.
    #[must_use]
    pub fn network_ids(&self) -> &[String] {
        &self.network_ids
    }
}

/// Host operations required to prove one worker generation is physically gone.
#[expect(
    async_fn_in_trait,
    reason = "workspace uses native async traits for injectable effect boundaries"
)]
pub trait WorkerCleanupEngine {
    /// Check that the recorded runner and `DinD` ids still match owned names/labels.
    async fn inspect_generation(
        &self,
        identity: &WorkerGenerationIdentity,
    ) -> Result<GenerationObservation, HostError>;

    /// Stop or verify the exact runner container under the requested policy.
    async fn stop_runner(
        &self,
        identity: &WorkerGenerationIdentity,
        policy: &RunnerStopPolicy,
    ) -> Result<RunnerStopEvidence, HostError>;

    /// Stop the exact private `DinD` container and verify its stopped state.
    ///
    /// Callers invoke this only after the runner is stopped, diagnostics are
    /// retained, and the durable private-child inventory is empty. An
    /// uncertain stop must return an error so the generation stays reserved.
    async fn stop_dind(
        &self,
        identity: &WorkerGenerationIdentity,
    ) -> Result<DindStopEvidence, HostError>;

    /// Download only the official runner's `_diag` archive, bounded in memory.
    async fn runner_diagnostics(
        &self,
        identity: &WorkerGenerationIdentity,
    ) -> Result<Option<Zeroizing<Vec<u8>>>, HostError>;

    /// Enumerate exact private-daemon child containers and custom networks.
    async fn list_dind_children(
        &self,
        identity: &WorkerGenerationIdentity,
    ) -> Result<ChildResourceInventory, HostError>;

    /// Remove one child id after the caller has durably recorded its intent.
    async fn remove_dind_child(
        &self,
        identity: &WorkerGenerationIdentity,
        id: &str,
        kind: ChildResourceKind,
    ) -> Result<(), HostError>;

    /// Remove and verify absence of the exact outer worker container.
    async fn remove_outer_container(
        &self,
        identity: &WorkerGenerationIdentity,
        role: OuterContainerRole,
    ) -> Result<(), HostError>;

    /// Remove the exact detached outer bridge owned by this generation.
    ///
    /// Legacy generations have no outer bridge and never call this operation.
    async fn remove_outer_network(
        &self,
        _identity: &WorkerGenerationIdentity,
    ) -> Result<(), HostError> {
        Err(HostError::Config)
    }

    /// Remove the deterministic, label-verified named-volume catalog.
    async fn remove_named_volumes(
        &self,
        identity: &WorkerGenerationIdentity,
    ) -> Result<Vec<String>, HostError>;
}

/// Whether a private child inventory includes custom resources.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChildResourceInventory {
    containers: Vec<String>,
    networks: Vec<String>,
}

impl ChildResourceInventory {
    /// Exact child container ids.
    #[must_use]
    pub fn containers(&self) -> &[String] {
        &self.containers
    }

    /// Exact user-created network ids.
    #[must_use]
    pub fn networks(&self) -> &[String] {
        &self.networks
    }
}

/// Outer container role used by exact cleanup operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OuterContainerRole {
    /// Official one-job runner container.
    Runner,
    /// Per-job private Docker-in-Docker container.
    Dind,
}

/// Initial state of the exact runner and private `DinD` containers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GenerationObservation {
    runner: ContainerObservation,
    dind: ContainerObservation,
}

impl GenerationObservation {
    /// State of the recorded runner container.
    #[must_use]
    pub const fn runner(&self) -> ContainerObservation {
        self.runner
    }

    /// State of the recorded private `DinD` container.
    #[must_use]
    pub const fn dind(&self) -> ContainerObservation {
        self.dind
    }
}

/// Whether a recorded container id still exists and is running.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ContainerObservation {
    /// Exact recorded id exists with expected ownership labels.
    pub present: bool,
    /// The exact container id has a live process state.
    pub running: bool,
    /// Exact Docker start-time interpretation; absent only if the container
    /// itself is absent. Unavailable start metadata maps to `MayHaveStarted`.
    pub started: Option<velnor_runner_journal::journal::RunnerStartObservation>,
}

fn docker_started_at_observation(
    started_at: Option<&str>,
) -> velnor_runner_journal::journal::RunnerStartObservation {
    use velnor_runner_journal::journal::RunnerStartObservation as Start;

    if started_at == Some("0001-01-01T00:00:00Z") {
        Start::NeverStarted
    } else {
        Start::MayHaveStarted
    }
}

/// Evidence from stopping the exact runner process.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RunnerStopEvidence {
    /// Runner process was proven stopped.
    pub stopped: bool,
    /// A kill was required after the graceful interval.
    pub forced: bool,
}

/// Evidence from stopping the exact private Docker daemon container.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DindStopEvidence {
    /// Docker inspection proved the exact container is absent or stopped.
    pub stopped: bool,
}
