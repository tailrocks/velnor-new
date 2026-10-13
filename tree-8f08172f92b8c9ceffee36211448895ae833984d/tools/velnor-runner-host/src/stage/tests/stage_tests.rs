//! Stage stops and recorded deletes drive `start_pair_until` and `remove_recorded`.

use std::collections::HashMap;
use std::sync::Mutex;

use super::super::{Forget, PairEngine, PairSink, PairStop, decide, drive};
use crate::HostError;
use crate::worker::CreateProjection;
use crate::worker::{WorkerNetworkFailure, WorkerNetworkPlan};

pub(super) fn fake_container_id(sequence: usize) -> String {
    format!("{sequence:064x}")
}

pub(super) fn fake_network_id() -> String {
    "e".repeat(64)
}

pub(super) struct Fake {
    events: Mutex<Vec<&'static str>>,
    ids: Mutex<Vec<String>>,
    names: Mutex<HashMap<String, String>>,
    removed: Mutex<Vec<String>>,
    specs: Mutex<Vec<CreateProjection>>,
    /// Fail the Nth call of this method name. `None` never fails.
    fail_at: Mutex<Option<(&'static str, u8)>>,
    counts: Mutex<HashMap<&'static str, u8>>,
}

impl Fake {
    pub(super) fn new() -> Self {
        Self {
            events: Mutex::new(Vec::new()),
            ids: Mutex::new(Vec::new()),
            names: Mutex::new(HashMap::new()),
            removed: Mutex::new(Vec::new()),
            specs: Mutex::new(Vec::new()),
            fail_at: Mutex::new(None),
            counts: Mutex::new(HashMap::new()),
        }
    }

    pub(super) fn events(&self) -> Vec<&'static str> {
        self.events
            .lock()
            .map(|guard| guard.clone())
            .unwrap_or_default()
    }

    pub(super) fn fail_on(&self, event: &'static str) {
        self.fail_on_nth(event, 1);
    }

    pub(super) fn fail_on_nth(&self, event: &'static str, nth: u8) {
        *self.fail_at.lock().expect("mutex") = Some((event, nth));
    }

    pub(super) fn removed(&self) -> Vec<String> {
        self.removed
            .lock()
            .map(|guard| guard.clone())
            .unwrap_or_default()
    }

    pub(super) fn specs(&self) -> Vec<CreateProjection> {
        self.specs
            .lock()
            .map(|guard| guard.clone())
            .unwrap_or_default()
    }

    fn hit(&self, event: &'static str) -> Result<(), HostError> {
        let mut counts = self.counts.lock().map_err(|_| HostError::Docker)?;
        let seen = counts.entry(event).or_insert(0);
        *seen = seen.saturating_add(1);
        let nth = *seen;
        drop(counts);
        let fail = self.fail_at.lock().map_err(|_| HostError::Docker)?;
        if let Some((name, at)) = *fail
            && name == event
            && nth == at
        {
            return Err(HostError::Docker);
        }
        Ok(())
    }
}

#[expect(
    clippy::unused_async_trait_impl,
    reason = "fake engine matches the async trait and does not await"
)]
impl PairEngine for Fake {
    async fn prepare_volumes(&self, _volume: &str) -> Result<(), HostError> {
        push(&self.events, "volumes")
    }

    async fn ensure_worker_network(
        &self,
        _plan: &WorkerNetworkPlan,
    ) -> Result<String, WorkerNetworkFailure> {
        self.hit("create-network")
            .map_err(|error| WorkerNetworkFailure::new(error, None, true))?;
        push(&self.events, "create-network")
            .map_err(|error| WorkerNetworkFailure::new(error, None, true))?;
        let id = fake_network_id();
        self.hit("network-inspect")
            .map_err(|error| WorkerNetworkFailure::new(error, Some(id.clone()), true))?;
        Ok(id)
    }

    async fn create(&self, spec: &CreateProjection) -> Result<String, HostError> {
        self.hit("create")?;
        push(&self.events, "create")?;
        self.specs
            .lock()
            .map_err(|_| HostError::Docker)?
            .push(spec.clone());
        let mut ids = self.ids.lock().map_err(|_| HostError::Docker)?;
        let id = fake_container_id(ids.len() + 1);
        ids.push(id.clone());
        drop(ids);
        self.names
            .lock()
            .map_err(|_| HostError::Docker)?
            .insert(spec.name.clone(), id.clone());
        Ok(id)
    }

    async fn start(&self, _id: &str) -> Result<(), HostError> {
        self.hit("start")?;
        push(&self.events, "start")
    }

    async fn write_jit(&self, _id: &str, _jit: &[u8]) -> Result<(), HostError> {
        self.hit("jit")?;
        push(&self.events, "jit")
    }

    async fn remove(&self, id: &str) -> Result<(), HostError> {
        push(&self.events, "remove")?;
        self.removed
            .lock()
            .map_err(|_| HostError::Docker)?
            .push(id.to_owned());
        let mut ids = self.ids.lock().map_err(|_| HostError::Docker)?;
        ids.retain(|kept| kept != id);
        Ok(())
    }

    async fn id_for_name(&self, name: &str) -> Result<Option<String>, HostError> {
        let ids = self.ids.lock().map_err(|_| HostError::Docker)?;
        if ids.iter().any(|id| id == name) {
            return Ok(Some(name.to_owned()));
        }
        let names = self.names.lock().map_err(|_| HostError::Docker)?;
        Ok(names.get(name).cloned())
    }

    async fn worker_id_for_name(
        &self,
        name: &str,
        _volume: &str,
        _role: &str,
    ) -> Result<Option<String>, HostError> {
        let names = self.names.lock().map_err(|_| HostError::Docker)?;
        Ok(names.get(name).cloned())
    }

    async fn remove_worker_volumes(&self, _volume: &str) -> Result<bool, HostError> {
        Ok(true)
    }

    async fn running(&self, _id: &str) -> Result<bool, HostError> {
        Ok(false)
    }
}

#[expect(
    clippy::unused_async_trait_impl,
    reason = "fake sink matches the async trait and records synchronously"
)]
impl PairSink for Fake {
    async fn volume(&self, _volume: &str) -> Result<(), HostError> {
        self.hit("sink-volume")?;
        push(&self.events, "sink-volume")
    }

    async fn dind(&self, _id: &str) -> Result<(), HostError> {
        self.hit("sink-dind")?;
        push(&self.events, "sink-dind")
    }

    async fn runner(&self, _id: &str) -> Result<(), HostError> {
        self.hit("sink-runner")?;
        push(&self.events, "sink-runner")
    }

    async fn outer_network_intent(&self, _name: &str) -> Result<(), HostError> {
        self.hit("network-intent")?;
        push(&self.events, "sink-network-intent")
    }

    async fn outer_network(&self, _id: &str) -> Result<(), HostError> {
        self.hit("network-id")?;
        push(&self.events, "sink-network-id")
    }

    async fn before_runner_start(
        &self,
        _id: &str,
        requirement: super::super::RunnerStartRequirement,
    ) -> Result<(), HostError> {
        self.hit("runner-start-intent")?;
        let event = match requirement {
            super::super::RunnerStartRequirement::LegacyCompatible => "sink-runner-start-legacy",
            super::super::RunnerStartRequirement::DurableRequired => "sink-runner-start-durable",
        };
        push(&self.events, event)
    }
}

fn push(events: &Mutex<Vec<&'static str>>, event: &'static str) -> Result<(), HostError> {
    events.lock().map_err(|_| HostError::Docker)?.push(event);
    Ok(())
}

#[tokio::test]
async fn dind_created_does_not_start() -> Result<(), HostError> {
    let engine = Fake::new();
    let partial = drive(&engine, "worker_a", b"jit", PairStop::DindCreated, &Forget).await?;
    assert_eq!(
        partial.dind_id.as_deref(),
        Some(fake_container_id(1).as_str())
    );
    assert_eq!(partial.runner_id, None);
    assert_eq!(engine.events(), ["volumes", "create"]);
    Ok(())
}

#[tokio::test]
async fn jit_stop_writes_stdin_after_both_starts() -> Result<(), HostError> {
    let engine = Fake::new();
    let partial = drive(&engine, "worker_a", b"jit", PairStop::Jit, &Forget).await?;
    assert!(partial.dind_id.is_some());
    assert!(partial.runner_id.is_some());
    assert_eq!(
        engine.events(),
        ["volumes", "create", "start", "create", "start", "jit"]
    );
    Ok(())
}

#[tokio::test]
async fn remove_recorded_deletes_only_the_owned_id() -> Result<(), HostError> {
    let engine = Fake::new();
    engine
        .names
        .lock()
        .map_err(|_| HostError::Docker)?
        .insert("runner".to_owned(), "aaaaaaaaaaaa".to_owned());
    let decision = decide(&engine, "aaaaaaaaaaaa", "runner").await?;
    assert_eq!(decision, crate::DeleteDecision::Delete);
    assert_eq!(engine.events(), ["remove"]);
    Ok(())
}

#[tokio::test]
async fn remove_recorded_keeps_a_foreign_id() -> Result<(), HostError> {
    let engine = Fake::new();
    engine
        .names
        .lock()
        .map_err(|_| HostError::Docker)?
        .insert("runner".to_owned(), "bbbbbbbbbbbb".to_owned());
    let decision = decide(&engine, "aaaaaaaaaaaa", "runner").await?;
    assert_eq!(decision, crate::DeleteDecision::KeepForeign);
    assert_eq!(engine.events(), Vec::<&str>::new());
    Ok(())
}

#[tokio::test]
async fn volumes_stop_creates_no_container() -> Result<(), HostError> {
    let engine = Fake::new();
    let partial = drive(&engine, "worker_a", b"jit", PairStop::Volumes, &Forget).await?;
    assert_eq!(partial.dind_id, None);
    assert_eq!(partial.runner_id, None);
    assert_eq!(engine.events(), ["volumes"]);
    Ok(())
}

#[tokio::test]
async fn dind_started_does_not_create_the_runner() -> Result<(), HostError> {
    let engine = Fake::new();
    let partial = drive(&engine, "worker_a", b"jit", PairStop::DindStarted, &Forget).await?;
    assert_eq!(
        partial.dind_id.as_deref(),
        Some(fake_container_id(1).as_str())
    );
    assert_eq!(partial.runner_id, None);
    assert_eq!(engine.events(), ["volumes", "create", "start"]);
    Ok(())
}

#[tokio::test]
async fn runner_created_does_not_start() -> Result<(), HostError> {
    let engine = Fake::new();
    let partial = drive(
        &engine,
        "worker_a",
        b"jit",
        PairStop::RunnerCreated,
        &Forget,
    )
    .await?;
    assert!(partial.dind_id.is_some() && partial.runner_id.is_some());
    assert_eq!(engine.events(), ["volumes", "create", "start", "create"]);
    Ok(())
}

#[tokio::test]
async fn second_create_failure_removes_only_the_owned_dind() -> Result<(), HostError> {
    let engine = Fake::new();
    *engine.fail_at.lock().map_err(|_| HostError::Docker)? = Some(("create", 2));
    engine
        .names
        .lock()
        .map_err(|_| HostError::Docker)?
        .insert("foreign".to_owned(), "bbbbbbbbbbbb".to_owned());
    let Err(error) = drive(&engine, "worker_a", b"jit", PairStop::Jit, &Forget).await else {
        return Err(HostError::Docker);
    };
    assert_eq!(error, HostError::Docker);
    assert_eq!(engine.removed(), [fake_container_id(1)]);
    assert_eq!(engine.events(), ["volumes", "create", "start", "remove"]);
    let names = engine.names.lock().map_err(|_| HostError::Docker)?;
    assert_eq!(
        names.get("foreign").map(String::as_str),
        Some("bbbbbbbbbbbb")
    );
    Ok(())
}

#[tokio::test]
async fn missing_name_is_not_deleted() -> Result<(), HostError> {
    let engine = Fake::new();
    let decision = decide(&engine, "aaaaaaaaaaaa", "absent").await?;
    assert_eq!(decision, crate::DeleteDecision::NotDeleted);
    assert_eq!(engine.events(), Vec::<&str>::new());
    Ok(())
}
