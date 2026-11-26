// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0
// SPDX-FileContributor: tailrocks/velnor-legacy@1b9fc0388925d21ff44ba1be5027684c38c8e4c9 crates/velnor-runner/src/runner.rs

use std::collections::BTreeMap;
use std::future::Future;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Context;
use time::OffsetDateTime;
use tokio::sync::Semaphore;
use tokio::sync::mpsc::UnboundedReceiver;
use tokio::task::{JoinHandle, JoinSet};
use tokio::time::{Instant, MissedTickBehavior, interval, timeout};
use uuid::Uuid;

use super::acquire::JobConnection;
use super::logs::{Event, LogFiles, iso_millis};
use super::status::StatusQueue;
use super::tail::StepState;
use crate::exec::{JobRun, StepResult};
use crate::job_message::AgentJobRequestMessage;
use crate::protocol::{
    FeedStreamClient, FeedWebSocket, StepConclusion, StepStatus, TwirpResultsClient, TwirpStep,
};

const CONNECT_BUDGET: Duration = Duration::from_secs(5);
const UPLOAD_SLOTS: usize = 4;
const FAST_FLUSH: Duration = Duration::from_millis(250);
const SLOW_FLUSH: Duration = Duration::from_millis(500);
const FAST_WINDOW: Duration = Duration::from_secs(60);
const FEED_BATCH_LINES: usize = 100;

#[derive(Debug, Clone, Default)]
pub(super) struct StepTimes {
    pub(super) started_at: Option<String>,
    pub(super) completed_at: Option<String>,
}

#[derive(Debug, Default)]
pub(super) struct ReportSummary {
    pub(super) times: BTreeMap<u32, StepTimes>,
}

pub(super) struct Reporter {
    results: Option<TwirpResultsClient>,
    feed_client: Option<FeedStreamClient>,
    feed: Option<FeedWebSocket>,
    plan_id: String,
    job_id: String,
    step_ids: Vec<String>,
    steps: BTreeMap<u32, StepState>,
    times: BTreeMap<u32, StepTimes>,
    files: LogFiles,
    budget: Duration,
    status: Option<StatusQueue>,
    upload_slots: Arc<Semaphore>,
    uploads: JoinSet<()>,
}

async fn warn_on_error<F>(label: &str, future: F)
where
    F: Future<Output = anyhow::Result<()>>,
{
    if let Err(error) = future.await {
        eprintln!("warning: {label}: {error:#}");
    }
}

fn conclusion(result: StepResult) -> StepConclusion {
    match result {
        StepResult::Succeeded => StepConclusion::Success,
        StepResult::Failed => StepConclusion::Failure,
        StepResult::Skipped => StepConclusion::Skipped,
        StepResult::Cancelled => StepConclusion::Cancelled,
    }
}

impl Reporter {
    pub(super) fn new(
        connection: &JobConnection,
        message: &AgentJobRequestMessage,
        files: LogFiles,
        budget: Duration,
    ) -> Self {
        let plan_id = message.plan.plan_id.clone();
        let job_id = message.job_id.clone();
        Self {
            status: connection.results.clone().map(|results| {
                StatusQueue::spawn(results, plan_id.clone(), job_id.clone(), budget)
            }),
            results: connection.results.clone(),
            feed_client: connection.feed.clone(),
            feed: None,
            plan_id,
            job_id,
            step_ids: message
                .steps
                .iter()
                .map(|step| {
                    step.id
                        .clone()
                        .unwrap_or_else(|| Uuid::new_v4().to_string())
                })
                .collect(),
            steps: BTreeMap::new(),
            times: BTreeMap::new(),
            files,
            budget,
            upload_slots: Arc::new(Semaphore::new(UPLOAD_SLOTS)),
            uploads: JoinSet::new(),
        }
    }

    pub(super) fn spawn(self, events: UnboundedReceiver<Event>) -> JoinHandle<ReportSummary> {
        tokio::spawn(self.run(events))
    }

    fn step_id(&self, number: u32) -> String {
        usize::try_from(number)
            .ok()
            .and_then(|number| number.checked_sub(1))
            .and_then(|index| self.step_ids.get(index))
            .cloned()
            .unwrap_or_default()
    }

    async fn run(mut self, mut events: UnboundedReceiver<Event>) -> ReportSummary {
        self.connect_feed().await;
        let started = Instant::now();
        let mut ticker = interval(FAST_FLUSH);
        ticker.set_missed_tick_behavior(MissedTickBehavior::Skip);
        let mut slow = false;
        loop {
            tokio::select! {
                event = events.recv() => match event {
                    Some(Event::JobFinished(run)) => {
                        self.finish(&run).await;
                        break;
                    }
                    Some(event) => self.handle(event).await,
                    None => break,
                },
                _ = ticker.tick() => {
                    self.flush_all().await;
                    if !slow && started.elapsed() > FAST_WINDOW {
                        slow = true;
                        ticker = interval(SLOW_FLUSH);
                        ticker.set_missed_tick_behavior(MissedTickBehavior::Skip);
                    }
                }
            }
        }
        self.close().await
    }

    async fn connect_feed(&mut self) {
        let Some(client) = &self.feed_client else {
            return;
        };
        match timeout(CONNECT_BUDGET, client.connect()).await {
            Ok(Ok(socket)) => self.feed = Some(socket),
            Ok(Err(error)) => eprintln!("warning: live feed unavailable: {error:#}"),
            Err(_) => eprintln!("warning: live feed unavailable: connect timed out"),
        }
    }

    async fn handle(&mut self, event: Event) {
        match event {
            Event::StepStarted { number, name, at } => {
                let started_at = iso_millis(at);
                let step = self.twirp_step(
                    number,
                    &name,
                    StepStatus::InProgress,
                    Some(&started_at),
                    None,
                    None,
                );
                self.steps
                    .insert(number, StepState::new(name, self.files.step_path(number)));
                self.times.insert(
                    number,
                    StepTimes {
                        started_at: Some(started_at),
                        completed_at: None,
                    },
                );
                self.update_steps(vec![step]);
            }
            Event::StepFinished { number, result, at } => {
                self.flush_step(number, true).await;
                self.finish_step(number, result, at);
            }
            Event::JobFinished(_) => {}
        }
    }

    fn twirp_step(
        &self,
        number: u32,
        name: &str,
        status: StepStatus,
        started_at: Option<&str>,
        completed_at: Option<&str>,
        result: Option<StepResult>,
    ) -> TwirpStep {
        TwirpStep {
            external_id: self.step_id(number),
            number: usize::try_from(number).unwrap_or(usize::MAX),
            name: name.to_owned(),
            status,
            started_at: started_at.map(ToOwned::to_owned),
            completed_at: completed_at.map(ToOwned::to_owned),
            conclusion: result.map_or(StepConclusion::Unknown, conclusion),
            is_background: false,
            background_control_type: None,
            background_control_step_ids: None,
            parallel_group_id: None,
        }
    }

    fn update_steps(&self, steps: Vec<TwirpStep>) {
        if let Some(status) = &self.status {
            status.push(steps);
        }
    }

    fn spawn_upload(
        &mut self,
        label: &'static str,
        upload: impl Future<Output = anyhow::Result<()>> + Send + 'static,
    ) {
        let slots = Arc::clone(&self.upload_slots);
        self.uploads.spawn(async move {
            let _slot = slots.acquire_owned().await;
            warn_on_error(label, upload).await;
        });
    }

    fn finish_step(&mut self, number: u32, result: StepResult, at: OffsetDateTime) {
        let completed_at = iso_millis(at);
        let started_at = self.times.get_mut(&number).and_then(|times| {
            times.completed_at = Some(completed_at.clone());
            times.started_at.clone()
        });
        let Some(state) = self.steps.remove(&number) else {
            return;
        };
        let step = self.twirp_step(
            number,
            &state.name,
            StepStatus::Completed,
            started_at.as_deref(),
            Some(&completed_at),
            Some(result),
        );
        self.update_steps(vec![step]);
        let Some(results) = self.results.clone() else {
            return;
        };
        let plan_id = self.plan_id.clone();
        let job_id = self.job_id.clone();
        let step_id = self.step_id(number);
        self.spawn_upload("upload step log", async move {
            results
                .upload_step_log(&plan_id, &job_id, &step_id, &state.path)
                .await
        });
    }

    async fn flush_all(&mut self) {
        let numbers: Vec<u32> = self.steps.keys().copied().collect();
        for number in numbers {
            self.flush_step(number, false).await;
        }
    }

    async fn flush_step(&mut self, number: u32, drain: bool) {
        if self.feed.is_none() {
            return;
        }
        let step_id = self.step_id(number);
        loop {
            let Some(state) = self.steps.get_mut(&number) else {
                return;
            };
            let lines = state.next_lines().await;
            if lines.is_empty() {
                return;
            }
            let mut start = state.taken + 1;
            state.taken += i64::try_from(lines.len()).unwrap_or(i64::MAX);
            let Some(mut socket) = self.feed.take() else {
                return;
            };
            for batch in lines.chunks(FEED_BATCH_LINES) {
                let sent = timeout(
                    self.budget,
                    FeedStreamClient::send_log_lines(&mut socket, &step_id, batch.to_vec(), start),
                )
                .await;
                if !matches!(sent, Ok(Ok(()))) {
                    eprintln!("warning: live feed stopped; step logs are still uploaded");
                    return;
                }
                start += i64::try_from(batch.len()).unwrap_or(i64::MAX);
            }
            self.feed = Some(socket);
            if !drain {
                return;
            }
        }
    }

    async fn finish(&mut self, run: &JobRun) {
        self.flush_all().await;
        let now = iso_millis(OffsetDateTime::now_utc());
        let mut late = Vec::new();
        for record in &run.steps {
            let announced = self.times.contains_key(&record.number);
            let times = self
                .times
                .entry(record.number)
                .or_insert_with(|| StepTimes {
                    started_at: None,
                    completed_at: Some(now.clone()),
                });
            if !announced || record.conclusion != record.outcome {
                let (started_at, completed_at) =
                    (times.started_at.clone(), times.completed_at.clone());
                late.push(self.twirp_step(
                    record.number,
                    &record.name,
                    StepStatus::Completed,
                    started_at.as_deref(),
                    completed_at.as_deref(),
                    Some(record.conclusion),
                ));
            }
            self.upload_summary(record.number, record.summary_path.as_deref());
        }
        if !late.is_empty() {
            self.update_steps(late);
        }
    }

    fn upload_summary(&mut self, number: u32, path: Option<&std::path::Path>) {
        let (Some(path), Some(results)) = (path, self.results.clone()) else {
            return;
        };
        let path = path.to_path_buf();
        let plan_id = self.plan_id.clone();
        let job_id = self.job_id.clone();
        let step_id = self.step_id(number);
        self.spawn_upload("upload step summary", async move {
            results
                .upload_step_summary(&plan_id, &job_id, &step_id, &path)
                .await
        });
    }

    async fn close(mut self) -> ReportSummary {
        while self.uploads.join_next().await.is_some() {}
        if let Some(results) = self.results.clone() {
            let (plan_id, job_id) = (self.plan_id.clone(), self.job_id.clone());
            let path = self.files.job_path();
            warn_on_error("upload job log", async move {
                let size = tokio::fs::metadata(&path)
                    .await
                    .with_context(|| format!("stat {}", path.display()))?
                    .len();
                if size == 0 {
                    return Ok(());
                }
                results.upload_job_log(&plan_id, &job_id, &path).await
            })
            .await;
        }
        if let Some(mut socket) = self.feed.take() {
            let _ = timeout(CONNECT_BUDGET, socket.close(None)).await;
        }
        if let Some(status) = self.status.take() {
            status.finish().await;
        }
        ReportSummary { times: self.times }
    }
}
