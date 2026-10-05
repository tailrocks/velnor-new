//! Completion cleanup runs outside the queue admission path.

use std::collections::HashMap;
use std::sync::{
    Arc, Condvar, Mutex,
    atomic::{AtomicBool, Ordering},
};
use std::time::Duration;

use velnor_runner_github::{
    Exchange, ImmutableJobContext, InnerJob, InnerKind, Method, ParsedBatch, Poll, SessionRequest,
    Transport, TransportFail,
};

use crate::action_archive_seed::ActionArchiveLease;
use crate::error::HostError;
use crate::journal::{Journal, LaunchIdentity, LaunchReservation, Outcome};
use crate::launch::completion;
use crate::launch_harness::open;
use crate::stage::{ContainerRecord, DindProbe, PairEngine};
use crate::worker::{CreateProjection, container_labels, container_name};

mod fair;
mod lease;
mod retry;
mod stalled;

async fn launch(
    journal: &Journal,
    set_id: i64,
    request_id: i64,
) -> Result<(i64, LaunchIdentity, String, String), String> {
    let LaunchReservation::New(id) = journal
        .reserve_assignment(set_id, request_id, request_id + 100, 8)
        .await
        .map_err(|error| error.to_string())?
    else {
        return Err("expected a new assignment".to_owned());
    };
    if !journal
        .claim_acquire(id)
        .await
        .map_err(|error| error.to_string())?
    {
        return Err("expected the acquire claim".to_owned());
    }
    journal
        .resolve_acquire(id, true)
        .await
        .map_err(|error| error.to_string())?;
    if !journal
        .claim_jit(id)
        .await
        .map_err(|error| error.to_string())?
    {
        return Err("expected the JIT claim".to_owned());
    }
    let identity = journal
        .launch_identity(id)
        .await
        .map_err(|error| error.to_string())?;
    let runner_id = format!("{:064x}", request_id + 1);
    let dind_id = format!("{:064x}", request_id + 2);
    let github_runner_id = (request_id + 10).to_string();
    journal
        .bind_github_runner(id, &github_runner_id)
        .await
        .map_err(|error| error.to_string())?;
    journal
        .bind_pair(id, &runner_id, &dind_id)
        .await
        .map_err(|error| error.to_string())?;
    journal
        .finish(id, Outcome::Done)
        .await
        .map_err(|error| error.to_string())?;
    Ok((id, identity, runner_id, dind_id))
}

fn completion_poll(request_id: i64, runner_id: i64, runner_name: &str) -> Poll {
    Poll::Batch(ParsedBatch {
        message_id: 12,
        statistics: None,
        jobs: vec![InnerJob {
            kind: InnerKind::Completed,
            request_id: Some(request_id),
            context: ImmutableJobContext {
                repository_name: None,
                owner_name: None,
                job_id: None,
                job_workflow_ref: None,
                job_display_name: None,
                workflow_run_id: None,
                event_name: None,
                request_labels: Vec::new(),
            },
            runner_id: Some(runner_id),
            runner_name: Some(runner_name.to_owned()),
            result: Some("Succeeded".to_owned()),
            fields: Vec::new(),
        }],
    })
}

async fn wait_for_lookup(api: &BlockingRunnerApi) -> Result<(), String> {
    tokio::time::timeout(Duration::from_secs(2), async {
        while !api.entered.load(Ordering::Acquire) {
            tokio::task::yield_now().await;
        }
    })
    .await
    .map_err(|_| "cleanup lookup did not start".to_owned())
}

#[derive(Clone)]
struct BlockingRunnerApi {
    entered: Arc<AtomicBool>,
    release: Arc<(Mutex<bool>, Condvar)>,
    runner_name: String,
    runner_id: i64,
    registered: Arc<AtomicBool>,
    get_count: Arc<std::sync::atomic::AtomicUsize>,
    calls: Arc<Mutex<Vec<(String, String, Option<String>)>>>,
}

impl BlockingRunnerApi {
    fn new(runner_name: &str, runner_id: i64) -> Self {
        Self {
            entered: Arc::new(AtomicBool::new(false)),
            release: Arc::new((Mutex::new(false), Condvar::new())),
            runner_name: runner_name.to_owned(),
            runner_id,
            registered: Arc::new(AtomicBool::new(true)),
            get_count: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
            calls: Arc::new(Mutex::new(Vec::new())),
        }
    }

    fn released(runner_name: &str, runner_id: i64) -> Self {
        let api = Self::new(runner_name, runner_id);
        api.release();
        api
    }

    fn release(&self) {
        if let Ok(mut released) = self.release.0.lock() {
            *released = true;
            self.release.1.notify_all();
        }
    }

    fn calls(&self) -> Result<Vec<(String, String, Option<String>)>, String> {
        self.calls
            .lock()
            .map(|calls| calls.clone())
            .map_err(|_| "runner API call log was poisoned".to_owned())
    }
}

impl Transport for BlockingRunnerApi {
    fn exchange(&mut self, request: &SessionRequest) -> Result<Exchange, TransportFail> {
        let method = match request.method {
            Method::Get => "GET",
            Method::Delete => "DELETE",
            Method::Post => "POST",
            Method::Patch => "PATCH",
        };
        self.calls.lock().map_err(|_| TransportFail::Reset)?.push((
            method.to_owned(),
            request.path.clone(),
            request.query.clone(),
        ));
        if request.method == Method::Delete {
            self.registered.store(false, Ordering::Release);
            return Ok(Exchange {
                status: 204,
                body: Vec::new(),
            });
        }
        if request.method != Method::Get {
            return Err(TransportFail::Http(500));
        }
        let request_number = self.get_count.fetch_add(1, Ordering::Relaxed);
        if request_number == 0 {
            self.entered.store(true, Ordering::Release);
            let (lock, ready) = &*self.release;
            let mut released = lock.lock().map_err(|_| TransportFail::Reset)?;
            while !*released {
                released = ready.wait(released).map_err(|_| TransportFail::Reset)?;
            }
        }
        if self.registered.load(Ordering::Acquire) {
            let body = serde_json::json!({
                "count": 1,
                "value": [{
                    "id": self.runner_id,
                    "name": self.runner_name,
                    "runnerScaleSetId": 7
                }]
            })
            .to_string()
            .into_bytes();
            return Ok(Exchange { status: 200, body });
        }
        Ok(Exchange {
            status: 200,
            body: br#"{"count":0,"value":[]}"#.to_vec(),
        })
    }
}

#[derive(Clone)]
struct CompletionEngine {
    identity: Option<LaunchIdentity>,
    records: Arc<Mutex<HashMap<String, ContainerRecord>>>,
    names: Arc<Mutex<HashMap<String, String>>>,
    removed_volumes: Arc<AtomicBool>,
    fail_volumes: Arc<AtomicBool>,
    delay_verify: Arc<AtomicBool>,
    verify_entered: Arc<AtomicBool>,
}

impl CompletionEngine {
    fn with_stopped_pair(
        identity: &LaunchIdentity,
        runner_id: &str,
        dind_id: &str,
    ) -> Result<Self, String> {
        let mut records = HashMap::new();
        let mut names = HashMap::new();
        for (role, id, running) in [("runner", runner_id, false), ("dind", dind_id, true)] {
            records.insert(
                id.to_owned(),
                ContainerRecord {
                    id: id.to_owned(),
                    labels: labels(identity, role),
                    running: Some(running),
                },
            );
            names.insert(container_name(identity, role), id.to_owned());
        }
        Ok(Self {
            identity: Some(identity.clone()),
            records: Arc::new(Mutex::new(records)),
            names: Arc::new(Mutex::new(names)),
            removed_volumes: Arc::new(AtomicBool::new(false)),
            fail_volumes: Arc::new(AtomicBool::new(false)),
            delay_verify: Arc::new(AtomicBool::new(false)),
            verify_entered: Arc::new(AtomicBool::new(false)),
        })
    }

    fn empty() -> Self {
        Self {
            identity: None,
            records: Arc::new(Mutex::new(HashMap::new())),
            names: Arc::new(Mutex::new(HashMap::new())),
            removed_volumes: Arc::new(AtomicBool::new(false)),
            fail_volumes: Arc::new(AtomicBool::new(false)),
            delay_verify: Arc::new(AtomicBool::new(false)),
            verify_entered: Arc::new(AtomicBool::new(false)),
        }
    }
}

#[expect(
    clippy::unused_async_trait_impl,
    reason = "the test engine has no external waits"
)]
impl PairEngine for CompletionEngine {
    async fn prepare_volumes(&self, _identity: &LaunchIdentity) -> Result<(), HostError> {
        Ok(())
    }
    async fn create(&self, _spec: &CreateProjection) -> Result<String, HostError> {
        Err(HostError::Docker)
    }
    async fn start(&self, _id: &str) -> Result<(), HostError> {
        Err(HostError::Docker)
    }
    async fn probe_dind(&self, _id: &str) -> Result<DindProbe, HostError> {
        Err(HostError::Docker)
    }

    async fn list_launch(
        &self,
        identity: &LaunchIdentity,
    ) -> Result<Vec<ContainerRecord>, HostError> {
        if self
            .identity
            .as_ref()
            .is_some_and(|expected| expected != identity)
        {
            return Err(HostError::Ownership);
        }
        self.records
            .lock()
            .map(|rows| rows.values().cloned().collect())
            .map_err(|_| HostError::Docker)
    }

    async fn verify_container(
        &self,
        identity: &LaunchIdentity,
        role: &str,
        id: &str,
        dind_id: Option<&str>,
        archive_lease: Option<&ActionArchiveLease>,
        require_running: bool,
    ) -> Result<ContainerRecord, HostError> {
        if self
            .identity
            .as_ref()
            .is_some_and(|expected| expected != identity)
            || (role == "runner" && dind_id.is_none())
            || archive_lease.is_some_and(|lease| lease.launch_id() != identity.launch_id())
        {
            return Err(HostError::Ownership);
        }
        let record = self
            .inspect_container(id)
            .await?
            .ok_or(HostError::Ownership)?;
        if record.labels != labels(identity, role)
            || (require_running && record.running != Some(true))
        {
            return Err(HostError::Ownership);
        }
        Ok(record)
    }

    async fn write_jit(&self, _id: &str, _jit: &[u8]) -> Result<(), HostError> {
        Err(HostError::Docker)
    }

    async fn remove(&self, id: &str) -> Result<(), HostError> {
        self.records
            .lock()
            .map_err(|_| HostError::Docker)?
            .remove(id)
            .ok_or(HostError::Ownership)?;
        self.names
            .lock()
            .map_err(|_| HostError::Docker)?
            .retain(|_, value| value != id);
        Ok(())
    }

    async fn inspect_container(
        &self,
        id_or_name: &str,
    ) -> Result<Option<ContainerRecord>, HostError> {
        let id = self
            .names
            .lock()
            .map_err(|_| HostError::Docker)?
            .get(id_or_name)
            .cloned()
            .unwrap_or_else(|| id_or_name.to_owned());
        Ok(self
            .records
            .lock()
            .map_err(|_| HostError::Docker)?
            .get(&id)
            .cloned())
    }

    async fn remove_volumes(&self, _identity: &LaunchIdentity) -> Result<(), HostError> {
        if self.fail_volumes.load(Ordering::Acquire) {
            return Err(HostError::Cleanup);
        }
        self.removed_volumes.store(true, Ordering::Release);
        Ok(())
    }

    async fn verify_engine(&self, identity: &LaunchIdentity) -> Result<(), HostError> {
        self.verify_entered.store(true, Ordering::Release);
        if self.delay_verify.load(Ordering::Acquire) {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        if self
            .identity
            .as_ref()
            .is_none_or(|expected| expected == identity)
        {
            Ok(())
        } else {
            Err(HostError::Ownership)
        }
    }
}

fn labels(identity: &LaunchIdentity, role: &str) -> HashMap<String, String> {
    container_labels(identity, role)
        .iter()
        .filter_map(|label| label.split_once('='))
        .map(|(key, value)| (key.to_owned(), value.to_owned()))
        .collect()
}
