// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use http_body_util::{BodyExt, Empty};
use hyper::Request;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};

use super::{ApiStub, Call};
use crate::io::reply::Reply;
use crate::rest::{DEFAULT_PER_PAGE, MAX_PER_PAGE, Page, runs};
use crate::standin;

impl ApiStub {
    async fn standin_bytes(&self, path: &str) -> Option<bytes::Bytes> {
        let request = Request::get(path)
            .body(Empty::new().map_err(|never| match never {}).boxed_unsync())
            .ok()?;
        let response = self.standin.as_ref()?.forward(request).await.ok()?;
        if !response.status().is_success() {
            return None;
        }
        Some(response.into_body().collect().await.ok()?.to_bytes())
    }

    async fn standin<T: DeserializeOwned>(&self, path: &str) -> Option<T> {
        serde_json::from_slice(&self.standin_bytes(path).await?).ok()
    }

    async fn records(&self, job: &standin::Job) -> Option<Vec<standin::Record>> {
        match &job.time_line_id {
            Some(id) => self.standin(&format!("/_apis/v1/Timeline/{id}")).await,
            None => Some(Vec::new()),
        }
    }

    async fn attempt_jobs(&self, id: u64, attempt: u32) -> Option<Vec<Value>> {
        let jobs: Vec<standin::Job> = self
            .standin(&format!("/_apis/v1/Message?runid={id}"))
            .await?;
        let mut listed = Vec::new();
        for job in jobs.iter().filter(|job| job.attempt == attempt) {
            listed.push(runs::job(job, &self.records(job).await?));
        }
        Some(listed)
    }

    pub(super) async fn run(&self, id: u64) -> Reply {
        let listed: Option<Vec<standin::Run>> =
            self.standin("/_apis/v1/Message/workflow/runs").await;
        let Some(run) = listed.and_then(|listed| listed.into_iter().find(|run| run.id == id))
        else {
            return Reply::missing();
        };
        match self.attempt_jobs(id, 1).await {
            Some(jobs) => Reply::ok(runs::run(&run, &jobs)),
            None => Reply::missing(),
        }
    }

    pub(super) async fn run_jobs(&self, call: &Call<'_>, id: u64, attempt: u32) -> Reply {
        let Some(jobs) = self.attempt_jobs(id, attempt).await else {
            return Reply::missing();
        };
        let page = Page::from_query(&call.params, DEFAULT_PER_PAGE, MAX_PER_PAGE);
        Reply::with(
            200,
            json!({"total_count": jobs.len(), "jobs": page.slice(&jobs)}),
            page.link(jobs.len(), call.host, call.path, &call.params),
        )
    }

    pub(super) async fn job_log(&self, id: u64) -> Option<String> {
        let jobs: Vec<standin::Job> = self.standin("/_apis/v1/Message").await?;
        let job = jobs.into_iter().find(|job| job.request_id == id)?;
        let mut log = String::new();
        for log_id in runs::log_ids(&self.records(&job).await?) {
            let bytes = self
                .standin_bytes(&format!("/_apis/v1/Logfiles/{log_id}"))
                .await?;
            log.push_str(&String::from_utf8_lossy(&bytes));
        }
        Some(log)
    }
}
