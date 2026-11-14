// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::io;

use serde_json::Value;
use tokio::fs;
use tokio::process::Command;

use super::Driver;
use crate::io::RESTORES;
use crate::io::env::request;
use crate::scenario::{Outcome, restores};
use crate::standin;

impl Driver<'_> {
    pub(super) async fn outcome(&self, run: standin::Run) -> io::Result<Outcome> {
        let metrics = self.env.dir.join(format!("metrics/{}.json", run.id));
        let id = run.id.to_string();
        let collect = [
            "bench",
            "collect",
            &id,
            "--repo",
            &self.args.repository,
            "-o",
        ];
        let jobs: Vec<standin::Job> = self.get(&format!("/_apis/v1/Message?runid={id}")).await?;
        let journal = match fs::read_to_string(self.env.dir.join("cache").join(RESTORES)).await {
            Err(error) if error.kind() == io::ErrorKind::NotFound => String::new(),
            read => read?,
        };
        let caches = restores(&journal, run.id, &jobs);
        let conclusion = standin::conclusion(run.result.as_deref()).unwrap_or("unfinished");
        let event = run.event_name.clone().unwrap_or_default();
        if self.fork {
            return Ok(Outcome {
                id: run.id,
                workflow: jobs
                    .first()
                    .map_or(run.file_name, |job| job.workflowname.clone()),
                event,
                conclusion,
                started: None,
                metrics: Err(String::new()),
                caches,
                trusted: false,
            });
        }
        let collected = Command::new(&self.args.velnor_ci)
            .kill_on_drop(true)
            .args(collect)
            .arg(&metrics)
            .env("GITHUB_API_URL", &self.addresses.stub_url)
            .env("GITHUB_TOKEN", &self.addresses.token)
            .output()
            .await;
        let written = fs::metadata(&metrics)
            .await
            .is_ok_and(|file| file.len() > 0);
        let path = format!("/repos/{}/actions/runs/{id}", self.args.repository);
        let bytes = request(&self.addresses.stub, "GET", &path, "")
            .await
            .ok_or_else(|| io::Error::other(format!("the API stub does not answer {path}")))?;
        let github: Value = serde_json::from_slice(&bytes).map_err(io::Error::other)?;
        Ok(Outcome {
            caches,
            trusted: true,
            id: run.id,
            workflow: github["name"].as_str().unwrap_or(&run.file_name).to_owned(),
            event,
            conclusion,
            started: github["run_started_at"].as_str().and_then(standin::seconds),
            metrics: match collected {
                Ok(done) if done.status.success() && written => Ok(metrics),
                Ok(done) if done.status.success() => Err(format!(
                    "{} exited 0 but wrote no {}",
                    self.args.velnor_ci.display(),
                    metrics.display()
                )),
                Ok(done) => Err(String::from_utf8_lossy(&done.stderr).trim().to_owned()),
                Err(error) => Err(format!("{}: {error}", self.args.velnor_ci.display())),
            },
        })
    }
}
