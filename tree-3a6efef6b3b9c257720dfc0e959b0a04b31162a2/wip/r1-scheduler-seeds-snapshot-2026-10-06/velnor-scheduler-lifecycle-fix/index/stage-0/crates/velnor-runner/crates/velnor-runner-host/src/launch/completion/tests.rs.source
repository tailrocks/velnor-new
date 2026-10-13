//! Completion cleanup runs outside the queue admission path.

use std::sync::{
    Arc, Condvar, Mutex,
    atomic::{AtomicBool, Ordering},
};
use std::time::Duration;

use velnor_runner_github::{
    Exchange, ImmutableJobContext, InnerJob, InnerKind, Method, ParsedBatch, Poll, SessionRequest,
    Transport, TransportFail,
};

use crate::journal::{Journal, LaunchIdentity, LaunchReservation, Outcome};
use crate::launch::completion;
use crate::launch_harness::open;

mod claim_fence;
mod docker_transport;
mod engine;
mod fair;
mod lease;
mod retry;
mod stalled;

use engine::CompletionEngine;

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
