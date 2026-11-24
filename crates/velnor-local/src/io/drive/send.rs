// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::io;
use std::time::Duration;

use serde::de::DeserializeOwned;
use serde_json::{Value, json};
use tokio::fs;

use super::Driver;
use super::refusal::CANCEL_GRACE;
use crate::cli::RunnerMode;
use crate::io::authority::Runners;
use crate::io::env::{Engine, Environment, STANDIN_IMAGE, client_log_tail, words};
use crate::io::job_logs;
use crate::io::probe::request;
use crate::io::supervisor::await_replacement;
use crate::pairs::{DECISIONS, consumed_agents};
use crate::side::Side;
use crate::standin;

const REPLACEMENT_TIMEOUT: Duration = Duration::from_secs(300);

impl<E: Engine + Copy + Send> Driver<'_, E> {
    async fn await_pairs(&self, side: Side) -> io::Result<()> {
        if self.args.environment.runner != RunnerMode::Pair {
            return Ok(());
        }
        let pairs = match side {
            Side::Trusted => self.args.environment.runners,
            Side::Untrusted => self.args.environment.fork_runners.max(1),
        };
        let runners =
            Runners::open(Environment::at(&self.env.dir, *self.env.engine()).await?).await?;
        let served = match fs::read_to_string(self.env.dir.join("runner").join(DECISIONS)).await {
            Err(error) if error.kind() == io::ErrorKind::NotFound => String::new(),
            read => read?,
        };
        let consumed = consumed_agents(&served).map_err(io::Error::other)?;
        await_replacement(&runners, side, pairs, &consumed, REPLACEMENT_TIMEOUT).await
    }

    pub(super) async fn get<T: DeserializeOwned>(&self, side: Side, path: &str) -> io::Result<T> {
        let proxy = self.addresses.standin(side)?;
        let bytes = request(proxy, "GET", path, "")
            .await
            .ok_or_else(|| io::Error::other(format!("runner.server does not answer {path}")))?;
        serde_json::from_slice(&bytes).map_err(io::Error::other)
    }

    async fn save_job_logs(&self, run: u64, payload: &Value) -> io::Result<()> {
        let values = crate::conformance::run_values(payload);
        let runs = self.env.dir.join("runs");
        fs::create_dir_all(&runs).await?;
        fs::write(
            runs.join(format!("{run}.json")),
            serde_json::to_string(&values).map_err(io::Error::other)?,
        )
        .await?;
        let path = format!("/_apis/v1/Message?runid={run}");
        let jobs: Vec<standin::Job> = self.get(Side::Trusted, &path).await?;
        let ids: Vec<u64> = jobs.iter().map(|job| job.request_id).collect();
        let stub = self.addresses.stub_url.trim_start_matches("http://");
        let logs = self.env.dir.join("logs");
        let missing = job_logs::save(
            stub,
            &self.addresses.token,
            &self.args.repository,
            run,
            &ids,
            &logs,
        )
        .await?;
        if !missing.is_empty() {
            println!(
                "velnor-local {}: no step log from the API stub for jobs {missing:?} of run {run}",
                self.tag
            );
        }
        Ok(())
    }

    pub(super) async fn send(
        &mut self,
        event: &str,
        payload: &Value,
        git_ref: &str,
        sha: &str,
    ) -> io::Result<()> {
        self.events += 1;
        let name = format!("{}-{event}", self.events);
        let dir = self.env.dir.display();
        fs::write(format!("{dir}/events/{name}.json"), payload.to_string()).await?;
        let runs = "/_apis/v1/Message/workflow/runs";
        let side = Side::of(payload);
        self.await_pairs(side).await?;
        let known: Vec<standin::Run> = self.get(side, runs).await?;
        let entry = json!({
            "id": name,
            "side": side,
            "event": event,
            "ref": git_ref,
            "sha": sha,
            "workflow": self.args.workflow,
        });
        request(&self.authority, "POST", "/ledger", &entry.to_string())
            .await
            .ok_or_else(|| io::Error::other(format!("the authority did not record {name}")))?;
        let (repo, events) = (
            format!("{}:/repo", self.repo.display()),
            format!("{dir}/events:/events:ro"),
        );
        let payload_path = format!("/events/{name}.json");
        let mut client = words("-w /repo --entrypoint dotnet");
        client.extend(["-v", &repo, "-v", &events]);
        client.push(STANDIN_IMAGE);
        let call = standin::ClientCall {
            event,
            payload: &payload_path,
            git_ref,
            sha,
            repository: &self.args.repository,
            workflow: self.args.workflow.as_deref(),
        };
        client.extend(call.args());
        let log = self.env.dir.join(format!("logs/{name}.log"));
        let finished = self.env.client(payload, &client, &log);
        tokio::pin!(finished);
        let succeeded = tokio::select! {
            done = &mut finished => done?,
            () = self.refused_after(side, event, &known) => {
                self.cancel_unfinished(side, &known).await;
                tokio::time::timeout(CANCEL_GRACE, finished)
                    .await
                    .ok()
                    .and_then(Result::ok)
                    .unwrap_or(false)
            }
        };
        let mut created = false;
        for run in self.get::<Vec<standin::Run>>(side, runs).await? {
            if known.iter().any(|old| old.id == run.id) {
                continue;
            }
            created = true;
            if standin::conclusion(run.result.as_deref()) == Some("skipped") {
                println!(
                    "velnor-local {}: {event} does not trigger {}",
                    self.tag, run.file_name
                );
                continue;
            }
            let id = run.id;
            let mut outcome = self.outcome(run, side).await?;
            if outcome.trusted && outcome.started.is_none() {
                outcome.client_log = Some(client_log_tail(&log).await?);
            }
            println!("velnor-local {}: {}", self.tag, outcome.line());
            self.outcomes.push(outcome);
            if side == Side::Trusted {
                self.save_job_logs(id, payload).await?;
            }
        }
        if !succeeded && !created {
            let tail = client_log_tail(&log).await?;
            return Err(io::Error::other(format!(
                "Runner.Client failed; see logs/{name}.log, last lines:\n{}",
                tail.join("\n")
            )));
        }
        Ok(())
    }
}
