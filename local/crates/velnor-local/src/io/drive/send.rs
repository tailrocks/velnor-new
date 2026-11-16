// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::io;

use serde::de::DeserializeOwned;
use serde_json::{Value, json};
use tokio::fs;

use super::Driver;
use crate::io::env::{STANDIN_IMAGE, words};
use crate::io::probe::request;
use crate::side::Side;
use crate::standin;

impl Driver<'_> {
    pub(super) async fn get<T: DeserializeOwned>(&self, side: Side, path: &str) -> io::Result<T> {
        let proxy = self.addresses.standin(side)?;
        let bytes = request(proxy, "GET", path, "")
            .await
            .ok_or_else(|| io::Error::other(format!("runner.server does not answer {path}")))?;
        serde_json::from_slice(&bytes).map_err(io::Error::other)
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
        let succeeded = self.env.client(payload, &client, &log).await?;
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
            let outcome = self.outcome(run, side).await?;
            println!("velnor-local {}: {}", self.tag, outcome.line());
            self.outcomes.push(outcome);
        }
        if !succeeded && !created {
            return Err(io::Error::other(format!(
                "Runner.Client failed; see logs/{name}.log"
            )));
        }
        Ok(())
    }
}
