//! Stage stops and recorded deletes drive `start_pair_until` and `remove_recorded`.

use std::collections::HashMap;
use std::sync::Mutex;

use super::{PairEngine, PairSink};
use crate::HostError;
use crate::worker::CreateProjection;

struct Fake {
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
    fn new() -> Self {
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

    fn events(&self) -> Vec<&'static str> {
        self.events
            .lock()
            .map(|guard| guard.clone())
            .unwrap_or_default()
    }

    fn removed(&self) -> Vec<String> {
        self.removed
            .lock()
            .map(|guard| guard.clone())
            .unwrap_or_default()
    }

    fn specs(&self) -> Vec<CreateProjection> {
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

    async fn create(&self, spec: &CreateProjection) -> Result<String, HostError> {
        self.hit("create")?;
        push(&self.events, "create")?;
        self.specs
            .lock()
            .map_err(|_| HostError::Docker)?
            .push(spec.clone());
        let mut ids = self.ids.lock().map_err(|_| HostError::Docker)?;
        let id = fake_id(ids.len() + 1);
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
}

fn push(events: &Mutex<Vec<&'static str>>, event: &'static str) -> Result<(), HostError> {
    events.lock().map_err(|_| HostError::Docker)?.push(event);
    Ok(())
}

fn fake_id(number: usize) -> String {
    format!("{number:064x}")
}

mod id_tests;
mod stage_tests;
