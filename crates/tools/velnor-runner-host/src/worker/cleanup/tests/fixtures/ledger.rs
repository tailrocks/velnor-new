use std::sync::Mutex;

use super::*;

#[derive(Default)]
pub(in crate::worker::cleanup::tests) struct FakeLedger {
    events: Mutex<Vec<String>>,
    children: Mutex<Vec<String>>,
    networks: Mutex<Vec<String>>,
    drained: Mutex<Option<ChildCleanupEvidence>>,
    runner_start: Mutex<Option<RunnerStartObservation>>,
    completed: Mutex<bool>,
    pub(super) refuse_begin: bool,
}

impl FakeLedger {
    pub(in crate::worker::cleanup::tests) fn refusing_begin() -> Self {
        Self {
            refuse_begin: true,
            ..Self::default()
        }
    }

    pub(in crate::worker::cleanup::tests) fn completed(&self) -> bool {
        *self.completed.lock().expect("mutex")
    }

    pub(in crate::worker::cleanup::tests) fn has_step(&self, event: &str) -> bool {
        self.events
            .lock()
            .expect("mutex")
            .iter()
            .any(|observed| observed == event)
    }

    pub(in crate::worker::cleanup::tests) fn has_drained_checkpoint(&self) -> bool {
        self.drained.lock().expect("mutex").is_some()
    }

    pub(in crate::worker::cleanup::tests) fn set_runner_start_observation(
        &self,
        observation: RunnerStartObservation,
    ) {
        *self.runner_start.lock().expect("mutex") = Some(observation);
    }
}

#[expect(
    clippy::unused_async_trait_impl,
    reason = "synchronous in-memory journal assertions implement the async persistence contract"
)]
impl CleanupLedger for FakeLedger {
    async fn begin(
        &self,
        _identity: &WorkerGenerationIdentity,
        _post_actions: &PostActionDisposition,
        _stop_policy: &RunnerStopPolicy,
    ) -> Result<(), HostError> {
        if self.refuse_begin {
            return Err(HostError::Identity);
        }
        self.events.lock().expect("mutex").push("begin".to_owned());
        Ok(())
    }

    async fn runner_start_observation(
        &self,
        _identity: &WorkerGenerationIdentity,
    ) -> Result<Option<RunnerStartObservation>, HostError> {
        Ok(self
            .runner_start
            .lock()
            .map_err(|_| HostError::Docker)?
            .as_ref()
            .copied())
    }

    async fn record_runner_start_observation(
        &self,
        _identity: &WorkerGenerationIdentity,
        observation: RunnerStartObservation,
    ) -> Result<(), HostError> {
        let mut state = self.runner_start.lock().map_err(|_| HostError::Docker)?;
        if state.as_ref().is_some_and(|prior| *prior != observation) {
            return Err(HostError::Identity);
        }
        *state = Some(observation);
        Ok(())
    }

    async fn before(&self, _launch_id: i64, step: &CleanupStep) -> Result<(), HostError> {
        self.events
            .lock()
            .expect("mutex")
            .push(format!("before-{step:?}"));
        Ok(())
    }

    async fn after(&self, _launch_id: i64, step: &CleanupStep) -> Result<(), HostError> {
        self.events
            .lock()
            .expect("mutex")
            .push(format!("after-{step:?}"));
        Ok(())
    }

    async fn observe_children(
        &self,
        _identity: &WorkerGenerationIdentity,
        inventory: &ChildResourceInventory,
    ) -> Result<(), HostError> {
        self.children
            .lock()
            .expect("mutex")
            .extend(inventory.containers().to_vec());
        self.networks
            .lock()
            .expect("mutex")
            .extend(inventory.networks().to_vec());
        Ok(())
    }

    async fn observed_children(
        &self,
        _identity: &WorkerGenerationIdentity,
    ) -> Result<ChildCleanupEvidence, HostError> {
        Ok(ChildCleanupEvidence {
            container_ids: self.children.lock().expect("mutex").clone(),
            network_ids: self.networks.lock().expect("mutex").clone(),
        })
    }

    async fn prior_children_drained(
        &self,
        _identity: &WorkerGenerationIdentity,
    ) -> Result<Option<ChildCleanupEvidence>, HostError> {
        Ok(self.drained.lock().expect("mutex").clone())
    }

    async fn children_drained(
        &self,
        _identity: &WorkerGenerationIdentity,
        evidence: &ChildCleanupEvidence,
    ) -> Result<(), HostError> {
        *self.drained.lock().expect("mutex") = Some(evidence.clone());
        Ok(())
    }

    async fn diagnostics(
        &self,
        _launch_id: i64,
        receipt: &DiagnosticsReceipt,
    ) -> Result<(), HostError> {
        if !receipt.redacted() || !receipt.retained() {
            return Err(HostError::Path);
        }
        Ok(())
    }

    async fn complete(&self, _proof: &WorkerTerminationProof) -> Result<(), HostError> {
        *self.completed.lock().expect("mutex") = true;
        Ok(())
    }
}
