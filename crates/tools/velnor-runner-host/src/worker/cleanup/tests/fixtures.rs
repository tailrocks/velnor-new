use std::sync::{Arc, Mutex};

use zeroize::Zeroizing;

mod diagnostics;
mod dind;

use crate::HostError;
use crate::worker::cleanup::*;
use crate::worker::volumes::worker_volume_names;
pub(super) use diagnostics::diagnostics_store;
use diagnostics::empty_diagnostic_tar;
use dind::fake_stop_dind;
use velnor_runner_journal::journal::RunnerStartObservation;

pub(super) const RUNNER_ID: &str = "a123456789abcdef";
pub(super) const DIND_ID: &str = "b123456789abcdef";
pub(super) const CHILD_ID: &str = "c123456789abcdef";
pub(super) const NETWORK_ID: &str = "d123456789abcdef";
pub(super) const OUTER_NETWORK_ID: &str = "e123456789abcdef";
pub(super) const WORKER: &str = "w0123456789abcdef0123456789abcdef";

pub(super) fn identity(
    job: Option<ObservedJobIdentity>,
) -> Result<WorkerGenerationIdentity, String> {
    WorkerGenerationIdentity::new(
        17,
        "velnor-17".to_owned(),
        WORKER.to_owned(),
        RUNNER_ID.to_owned(),
        DIND_ID.to_owned(),
        job,
    )
    .map_err(|error| error.to_string())
}

pub(super) fn identity_with_outer_network(
    job: Option<ObservedJobIdentity>,
) -> Result<WorkerGenerationIdentity, String> {
    WorkerGenerationIdentity::new_with_network(
        17,
        "velnor-17".to_owned(),
        WORKER.to_owned(),
        RUNNER_ID.to_owned(),
        DIND_ID.to_owned(),
        Some(format!("{WORKER}-outer")),
        Some(OUTER_NETWORK_ID.to_owned()),
        job,
    )
    .map_err(|error| error.to_string())
}

pub(super) fn index(events: &[String], prefix: &str) -> usize {
    events
        .iter()
        .position(|event| event.starts_with(prefix))
        .expect("expected cleanup event")
}

#[derive(Clone)]
pub(super) struct FakeEngine {
    events: Arc<Mutex<Vec<String>>>,
    runner_present: Arc<Mutex<bool>>,
    runner_running: Arc<Mutex<bool>>,
    runner_started: Arc<Mutex<bool>>,
    dind_present: Arc<Mutex<bool>>,
    dind_running: Arc<Mutex<bool>>,
    dind_started: Arc<Mutex<RunnerStartObservation>>,
    children: Arc<Mutex<Vec<String>>>,
    networks: Arc<Mutex<Vec<String>>>,
    fail_volume_cleanup: Arc<Mutex<bool>>,
    fail_child_after_remove: Arc<Mutex<bool>>,
    fail_outer_removal: Arc<Mutex<bool>>,
    fail_dind_stop_before_effect: Arc<Mutex<bool>>,
    lose_dind_stop_response: Arc<Mutex<bool>>,
    lose_runner_stop_response: Arc<Mutex<bool>>,
    fail_network_removal: Arc<Mutex<bool>>,
    force_runner_stop: Arc<Mutex<bool>>,
    outer_network_present: Arc<Mutex<bool>>,
    diagnostics: Option<Vec<u8>>,
}

impl FakeEngine {
    pub(super) fn running() -> Self {
        Self::running_with_diagnostics(Some(empty_diagnostic_tar()))
    }

    pub(super) fn running_with_diagnostics(diagnostics: Option<Vec<u8>>) -> Self {
        Self {
            events: Arc::default(),
            runner_present: Arc::new(Mutex::new(true)),
            runner_running: Arc::new(Mutex::new(false)),
            runner_started: Arc::new(Mutex::new(true)),
            dind_present: Arc::new(Mutex::new(true)),
            dind_running: Arc::new(Mutex::new(true)),
            dind_started: Arc::new(Mutex::new(RunnerStartObservation::MayHaveStarted)),
            children: Arc::new(Mutex::new(vec![CHILD_ID.to_owned()])),
            networks: Arc::new(Mutex::new(vec![NETWORK_ID.to_owned()])),
            fail_volume_cleanup: Arc::new(Mutex::new(false)),
            fail_child_after_remove: Arc::new(Mutex::new(false)),
            fail_outer_removal: Arc::new(Mutex::new(false)),
            fail_dind_stop_before_effect: Arc::new(Mutex::new(false)),
            lose_dind_stop_response: Arc::new(Mutex::new(false)),
            lose_runner_stop_response: Arc::new(Mutex::new(false)),
            fail_network_removal: Arc::new(Mutex::new(false)),
            force_runner_stop: Arc::new(Mutex::new(false)),
            outer_network_present: Arc::new(Mutex::new(false)),
            diagnostics,
        }
    }

    pub(super) fn running_with_outer_network() -> Self {
        let engine = Self::running();
        *engine.outer_network_present.lock().expect("mutex") = true;
        engine
    }

    pub(super) fn running_runner() -> Self {
        let engine = Self::running();
        *engine.runner_running.lock().expect("mutex") = true;
        engine
    }

    pub(super) fn absent_runner_without_diagnostics() -> Self {
        let engine = Self::running_with_diagnostics(None);
        *engine.runner_present.lock().expect("mutex") = false;
        engine
    }

    pub(super) fn never_started() -> Self {
        let engine = Self::running_with_diagnostics(None);
        *engine.runner_started.lock().expect("mutex") = false;
        engine
    }

    pub(super) fn never_started_dind() -> Self {
        let engine = Self::never_started();
        *engine.dind_running.lock().expect("mutex") = false;
        *engine.dind_started.lock().expect("mutex") = RunnerStartObservation::NeverStarted;
        engine.children.lock().expect("mutex").clear();
        engine.networks.lock().expect("mutex").clear();
        engine
    }

    pub(super) fn stopped_after_dind_start() -> Self {
        let engine = Self::running();
        *engine.dind_running.lock().expect("mutex") = false;
        engine
    }

    pub(super) fn running_with_forced_stop() -> Self {
        let engine = Self::running_runner();
        *engine.force_runner_stop.lock().expect("mutex") = true;
        engine
    }

    pub(super) fn running_with_lost_runner_stop_response() -> Self {
        let engine = Self::running_runner();
        *engine.lose_runner_stop_response.lock().expect("mutex") = true;
        engine
    }

    pub(super) fn running_with_volume_failure() -> Self {
        let engine = Self::running();
        *engine.fail_volume_cleanup.lock().expect("mutex") = true;
        engine
    }

    pub(super) fn running_with_lost_child_response() -> Self {
        let engine = Self::running();
        *engine.fail_child_after_remove.lock().expect("mutex") = true;
        engine
    }

    pub(super) fn running_with_outer_removal_failure() -> Self {
        let engine = Self::running();
        *engine.fail_outer_removal.lock().expect("mutex") = true;
        engine
    }

    pub(super) fn running_with_network_removal_failure() -> Self {
        let engine = Self::running_with_outer_network();
        *engine.fail_network_removal.lock().expect("mutex") = true;
        engine
    }

    pub(super) fn events(&self) -> Vec<String> {
        self.events.lock().expect("mutex").clone()
    }

    pub(super) fn is_absent(&self, role: OuterContainerRole) -> bool {
        match role {
            OuterContainerRole::Runner => !*self.runner_present.lock().expect("mutex"),
            OuterContainerRole::Dind => !*self.dind_present.lock().expect("mutex"),
        }
    }

    pub(super) fn runner_is_running(&self) -> bool {
        *self.runner_running.lock().expect("mutex")
    }

    pub(super) fn outer_network_is_absent(&self) -> bool {
        !*self.outer_network_present.lock().expect("mutex")
    }

    pub(super) fn allow_volume_cleanup(&self) {
        *self.fail_volume_cleanup.lock().expect("mutex") = false;
    }

    pub(super) fn add_child_container(&self, id: &str) {
        self.children.lock().expect("mutex").push(id.to_owned());
    }

    fn push(&self, event: impl Into<String>) {
        self.events.lock().expect("mutex").push(event.into());
    }
}

#[expect(
    clippy::unused_async_trait_impl,
    reason = "deterministic fake state transitions implement the production async effects"
)]
impl WorkerCleanupEngine for FakeEngine {
    async fn inspect_generation(
        &self,
        _identity: &WorkerGenerationIdentity,
    ) -> Result<GenerationObservation, HostError> {
        let runner_present = *self.runner_present.lock().expect("mutex");
        let runner_running = *self.runner_running.lock().expect("mutex");
        let runner_started = *self.runner_started.lock().expect("mutex");
        let dind_present = *self.dind_present.lock().expect("mutex");
        let dind_running = *self.dind_running.lock().expect("mutex");
        let dind_started = *self.dind_started.lock().expect("mutex");
        Ok(GenerationObservation {
            runner: ContainerObservation {
                present: runner_present,
                running: runner_running,
                started: runner_present.then_some(if runner_started {
                    RunnerStartObservation::MayHaveStarted
                } else {
                    RunnerStartObservation::NeverStarted
                }),
            },
            dind: ContainerObservation {
                present: dind_present,
                running: dind_present && dind_running,
                started: dind_present.then_some(dind_started),
            },
        })
    }

    async fn stop_runner(
        &self,
        identity: &WorkerGenerationIdentity,
        policy: &RunnerStopPolicy,
    ) -> Result<RunnerStopEvidence, HostError> {
        if matches!(policy, RunnerStopPolicy::RequireStopped) {
            return Err(HostError::Docker);
        }
        *self.runner_running.lock().expect("mutex") = false;
        self.push(format!("stop-runner-{}", identity.runner_container_id()));
        let forced = *self.force_runner_stop.lock().expect("mutex");
        let response_lost = {
            let mut fail = self.lose_runner_stop_response.lock().expect("mutex");
            let value = *fail;
            *fail = false;
            value
        };
        if response_lost {
            Err(HostError::Docker)
        } else {
            Ok(RunnerStopEvidence {
                stopped: true,
                forced,
            })
        }
    }

    async fn stop_dind(
        &self,
        _identity: &WorkerGenerationIdentity,
    ) -> Result<DindStopEvidence, HostError> {
        fake_stop_dind(self).await
    }

    async fn runner_diagnostics(
        &self,
        _identity: &WorkerGenerationIdentity,
    ) -> Result<Option<Zeroizing<Vec<u8>>>, HostError> {
        self.push("diagnostics");
        Ok(self.diagnostics.clone().map(Zeroizing::new))
    }

    async fn list_dind_children(
        &self,
        _identity: &WorkerGenerationIdentity,
    ) -> Result<ChildResourceInventory, HostError> {
        let containers = self.children.lock().expect("mutex").clone();
        let networks = self.networks.lock().expect("mutex").clone();
        if containers.is_empty() && networks.is_empty() {
            self.push("children-empty");
        }
        Ok(ChildResourceInventory {
            containers,
            networks,
        })
    }

    async fn remove_dind_child(
        &self,
        _identity: &WorkerGenerationIdentity,
        id: &str,
        kind: ChildResourceKind,
    ) -> Result<(), HostError> {
        self.push(format!("remove-child-{kind:?}-{id}"));
        match kind {
            ChildResourceKind::Container => self
                .children
                .lock()
                .expect("mutex")
                .retain(|observed| observed != id),
            ChildResourceKind::Network => self
                .networks
                .lock()
                .expect("mutex")
                .retain(|observed| observed != id),
        }
        let response_lost = {
            let mut fail = self.fail_child_after_remove.lock().expect("mutex");
            let value = *fail;
            *fail = false;
            value
        };
        if response_lost {
            Err(HostError::Docker)
        } else {
            Ok(())
        }
    }

    async fn remove_outer_container(
        &self,
        _identity: &WorkerGenerationIdentity,
        role: OuterContainerRole,
    ) -> Result<(), HostError> {
        self.push(match role {
            OuterContainerRole::Runner => "remove-runner",
            OuterContainerRole::Dind => "remove-dind",
        });
        if role == OuterContainerRole::Runner {
            let mut fail = self.fail_outer_removal.lock().expect("mutex");
            if *fail {
                *fail = false;
                return Err(HostError::Docker);
            }
        }
        match role {
            OuterContainerRole::Runner => *self.runner_present.lock().expect("mutex") = false,
            OuterContainerRole::Dind => {
                *self.dind_present.lock().expect("mutex") = false;
                *self.dind_running.lock().expect("mutex") = false;
            }
        }
        Ok(())
    }

    async fn remove_outer_network(
        &self,
        identity: &WorkerGenerationIdentity,
    ) -> Result<(), HostError> {
        self.push("remove-outer-network");
        if identity.outer_network_name().is_none()
            || identity.outer_network_id().is_none()
            || !*self.outer_network_present.lock().expect("mutex")
        {
            return Err(HostError::Identity);
        }
        let mut fail = self.fail_network_removal.lock().expect("mutex");
        if *fail {
            *fail = false;
            return Err(HostError::Docker);
        }
        *self.outer_network_present.lock().expect("mutex") = false;
        Ok(())
    }

    async fn remove_named_volumes(
        &self,
        identity: &WorkerGenerationIdentity,
    ) -> Result<Vec<String>, HostError> {
        self.push("remove-volumes");
        if *self.fail_volume_cleanup.lock().expect("mutex") {
            return Err(HostError::Docker);
        }
        worker_volume_names(identity.worker_volume())
    }
}

mod ledger;
pub(super) use ledger::FakeLedger;
