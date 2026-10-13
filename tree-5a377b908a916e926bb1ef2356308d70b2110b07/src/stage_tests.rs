//! Stage stops and recorded deletes drive `start_pair_until` and `remove_recorded`.

use std::collections::HashMap;
use std::sync::Mutex;

use super::HostError;
use super::stage::{PairEngine, PairStop, decide, drive};
use super::worker::CreateProjection;

struct Fake {
    events: Mutex<Vec<&'static str>>,
    ids: Mutex<Vec<String>>,
    names: Mutex<HashMap<String, String>>,
    removed: Mutex<Vec<String>>,
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

    async fn create(&self, _spec: &CreateProjection) -> Result<String, HostError> {
        self.hit("create")?;
        push(&self.events, "create")?;
        let mut ids = self.ids.lock().map_err(|_| HostError::Docker)?;
        let id = format!("{:012x}", ids.len() + 1);
        ids.push(id.clone());
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
}

fn push(events: &Mutex<Vec<&'static str>>, event: &'static str) -> Result<(), HostError> {
    events.lock().map_err(|_| HostError::Docker)?.push(event);
    Ok(())
}

#[tokio::test]
async fn dind_created_does_not_start() -> Result<(), HostError> {
    let engine = Fake::new();
    let partial = drive(&engine, "worker_a", b"jit", PairStop::DindCreated).await?;
    assert_eq!(partial.dind_id.as_deref(), Some("000000000001"));
    assert_eq!(partial.runner_id, None);
    assert_eq!(engine.events(), ["volumes", "create"]);
    Ok(())
}

#[tokio::test]
async fn jit_stop_writes_stdin_after_both_starts() -> Result<(), HostError> {
    let engine = Fake::new();
    let partial = drive(&engine, "worker_a", b"jit", PairStop::Jit).await?;
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
    assert_eq!(decision, super::DeleteDecision::Delete);
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
    assert_eq!(decision, super::DeleteDecision::KeepForeign);
    assert_eq!(engine.events(), Vec::<&str>::new());
    Ok(())
}

#[tokio::test]
async fn volumes_stop_creates_no_container() -> Result<(), HostError> {
    let engine = Fake::new();
    let partial = drive(&engine, "worker_a", b"jit", PairStop::Volumes).await?;
    assert_eq!(partial.dind_id, None);
    assert_eq!(partial.runner_id, None);
    assert_eq!(engine.events(), ["volumes"]);
    Ok(())
}

#[tokio::test]
async fn dind_started_does_not_create_the_runner() -> Result<(), HostError> {
    let engine = Fake::new();
    let partial = drive(&engine, "worker_a", b"jit", PairStop::DindStarted).await?;
    assert_eq!(partial.dind_id.as_deref(), Some("000000000001"));
    assert_eq!(partial.runner_id, None);
    assert_eq!(engine.events(), ["volumes", "create", "start"]);
    Ok(())
}

#[tokio::test]
async fn runner_created_does_not_start() -> Result<(), HostError> {
    let engine = Fake::new();
    let partial = drive(&engine, "worker_a", b"jit", PairStop::RunnerCreated).await?;
    assert!(partial.dind_id.is_some() && partial.runner_id.is_some());
    assert_eq!(engine.events(), ["volumes", "create", "start", "create"]);
    Ok(())
}

#[tokio::test]
async fn runner_started_does_not_write_jit() -> Result<(), HostError> {
    let engine = Fake::new();
    let partial = drive(&engine, "worker_a", b"jit", PairStop::RunnerStarted).await?;
    assert!(partial.runner_id.is_some());
    assert_eq!(
        engine.events(),
        ["volumes", "create", "start", "create", "start"]
    );
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
    let Err(error) = drive(&engine, "worker_a", b"jit", PairStop::Jit).await else {
        return Err(HostError::Docker);
    };
    assert_eq!(error, HostError::Docker);
    assert_eq!(engine.removed(), ["000000000001".to_owned()]);
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
    assert_eq!(decision, super::DeleteDecision::NotDeleted);
    assert_eq!(engine.events(), Vec::<&str>::new());
    Ok(())
}
