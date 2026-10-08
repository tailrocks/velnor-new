use super::PartialPair;
use crate::HostError;

/// Stage where a durable sink-aware pair attempt stopped.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PairStartPhase {
    /// Input or immutable image profile validation failed before Docker work.
    Preflight,
    /// Durable worker-volume intent failed before volume preparation.
    VolumeIntent,
    /// Volume preparation may have partially completed.
    VolumePreparation,
    /// Durable outer-network intent failed before network creation.
    NetworkIntent,
    /// Network creation may have completed despite an error response.
    NetworkCreation,
    /// Persisting the created outer-network id failed.
    NetworkIdentity,
    /// Container creation may have completed despite an error response.
    DindCreation,
    /// Persisting the created `DinD` id failed.
    DindIdentity,
    /// Starting `DinD` may have completed despite an error response.
    DindStart,
    /// The runner projection was rejected after `DinD` existed.
    RunnerProjection,
    /// Runner creation may have completed despite an error response.
    RunnerCreation,
    /// Persisting the created runner id failed.
    RunnerIdentity,
    /// Durable runner-start intent failed before runner start.
    RunnerStartIntent,
    /// Starting the runner may have completed despite an error response.
    RunnerStart,
    /// Writing JIT may have completed despite an error response.
    JitWrite,
    /// The pair reached a stage that violated the public result invariant.
    ResultInvariant,
}

/// A sink-aware launch failure with the exact resources known at that point.
///
/// Production launch callers must retain this partial identity for journal
/// reconciliation. The failure does not authorize an automatic retry or
/// deletion because a Docker response can be lost after the side effect.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("worker pair staging stopped during {phase:?}: {error}")]
pub struct PairStartFailure {
    error: HostError,
    partial: PartialPair,
    phase: PairStartPhase,
    side_effect_may_have_succeeded: bool,
}

impl PairStartFailure {
    pub(super) fn new(
        error: HostError,
        partial: PartialPair,
        phase: PairStartPhase,
        side_effect_may_have_succeeded: bool,
    ) -> Self {
        Self {
            error,
            partial,
            phase,
            side_effect_may_have_succeeded,
        }
    }

    pub(crate) fn incomplete_result(partial: PartialPair) -> Self {
        Self::new(
            HostError::Docker,
            partial,
            PairStartPhase::ResultInvariant,
            true,
        )
    }

    /// Original controller or Docker error, without a secret-bearing payload.
    #[must_use]
    pub fn error(&self) -> HostError {
        self.error
    }

    /// Exact Docker ids received before the failure. Missing ids remain
    /// unresolved and must be reconciled by deterministic name and labels.
    #[must_use]
    pub fn partial(&self) -> &PartialPair {
        &self.partial
    }

    /// Failed staging boundary.
    #[must_use]
    pub fn phase(&self) -> PairStartPhase {
        self.phase
    }

    /// Whether an external side effect may exist after this error.
    #[must_use]
    pub fn side_effect_may_have_succeeded(&self) -> bool {
        self.side_effect_may_have_succeeded
    }
}

/// Records durable identities before the next external start.
#[expect(
    async_fn_in_trait,
    reason = "workspace style is async traits; RPITIT migration is a separate decision"
)]
pub trait PairSink {
    /// Record the worker volume before any container exists.
    async fn volume(&self, volume: &str) -> Result<(), HostError>;
    /// Record the dind container id.
    async fn dind(&self, id: &str) -> Result<(), HostError>;
    /// Record the runner container id.
    async fn runner(&self, id: &str) -> Result<(), HostError>;
    /// Persist the deterministic outer-network name before Docker create.
    async fn outer_network_intent(&self, _name: &str) -> Result<(), HostError> {
        Err(HostError::Config)
    }
    /// Persist the returned network id before attaching a worker container.
    async fn outer_network(&self, _id: &str) -> Result<(), HostError> {
        Err(HostError::Config)
    }
    /// Persist intent immediately before the runner is started.
    ///
    /// The default preserves unjournaled legacy behavior but rejects a
    /// profiled start. A durable launch sink overrides this callback.
    async fn before_runner_start(
        &self,
        _id: &str,
        requirement: RunnerStartRequirement,
    ) -> Result<(), HostError> {
        match requirement {
            RunnerStartRequirement::LegacyCompatible => Ok(()),
            RunnerStartRequirement::DurableRequired => Err(HostError::Config),
        }
    }
}

/// Persistence requirement at the runner-start boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunnerStartRequirement {
    /// Keep existing unjournaled legacy behavior, including macOS.
    LegacyCompatible,
    /// Require a durable launch-owned start-intent write.
    DurableRequired,
}

/// A sink that does not record ids.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Forget;

#[expect(
    clippy::unused_async_trait_impl,
    reason = "sink matches the async trait and does not await"
)]
impl PairSink for Forget {
    async fn volume(&self, _volume: &str) -> Result<(), HostError> {
        Ok(())
    }

    async fn dind(&self, _id: &str) -> Result<(), HostError> {
        Ok(())
    }

    async fn runner(&self, _id: &str) -> Result<(), HostError> {
        Ok(())
    }
}
