// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0
// SPDX-FileContributor: tailrocks/velnor-legacy@1b9fc0388925d21ff44ba1be5027684c38c8e4c9 crates/velnor-runner/src/protocol.rs

use anyhow::{Context, Result};
use reqwest::Method;
use serde::{Deserialize, Serialize};
use std::time::Duration;
use url::Url;

use super::error::github_api_error;
use super::http::{github_json_request, slash_url, validate_known_service_url};
use super::timeline::{TimelineRecord, TimelineRecordFeedLines};

const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
const API_VERSION: &str = "5.1-preview.1";
const KNOWN_HOSTS: &[&str] = &["pipelines.actions.githubusercontent.com"];

#[derive(Debug, Clone)]
pub struct DistributedTaskClient {
    server_root_url: Url,
    bearer_token: String,
}

#[derive(Serialize)]
struct CollectionRequest<'a> {
    value: &'a [TimelineRecord],
}

#[derive(Deserialize)]
#[serde(untagged)]
enum CollectionResponse {
    Wrapped { value: Vec<TimelineRecord> },
    Bare(Vec<TimelineRecord>),
}

impl DistributedTaskClient {
    pub fn new(server_url: &str, bearer_token: impl Into<String>) -> Result<Self> {
        let url = validate_known_service_url(server_url, "SystemVssConnection", KNOWN_HOSTS)?;
        Ok(Self {
            server_root_url: slash_url(&url),
            bearer_token: bearer_token.into(),
        })
    }

    pub async fn update_timeline_records(
        &self,
        scope_identifier: &str,
        hub_name: &str,
        plan_id: &str,
        timeline_id: &str,
        records: &[TimelineRecord],
    ) -> Result<Vec<TimelineRecord>> {
        let url = self.timeline_url(scope_identifier, hub_name, plan_id, timeline_id, "records")?;
        let body = serde_json::to_string(&CollectionRequest { value: records })
            .context("serialize timeline records")?;
        let (status, text) = github_json_request(
            Method::PATCH,
            url.as_str(),
            &self.bearer_token,
            Some(body),
            REQUEST_TIMEOUT,
        )
        .await?;
        if !(200..300).contains(&status) {
            return Err(github_api_error("update timeline records", status, &text));
        }
        match serde_json::from_str(&text).context("parse update timeline records response")? {
            CollectionResponse::Wrapped { value } | CollectionResponse::Bare(value) => Ok(value),
        }
    }

    pub async fn append_timeline_record_feed(
        &self,
        scope_identifier: &str,
        hub_name: &str,
        plan_id: &str,
        timeline_id: &str,
        record_id: &str,
        feed: &TimelineRecordFeedLines,
    ) -> Result<()> {
        let url = self.timeline_url(
            scope_identifier,
            hub_name,
            plan_id,
            timeline_id,
            &format!("records/{record_id}/feed"),
        )?;
        let body = serde_json::to_string(feed).context("serialize timeline record feed")?;
        let (status, text) = github_json_request(
            Method::POST,
            url.as_str(),
            &self.bearer_token,
            Some(body),
            REQUEST_TIMEOUT,
        )
        .await?;
        if !(200..300).contains(&status) {
            return Err(github_api_error(
                "append timeline record feed",
                status,
                &text,
            ));
        }
        Ok(())
    }

    fn timeline_url(
        &self,
        scope_identifier: &str,
        hub_name: &str,
        plan_id: &str,
        timeline_id: &str,
        tail: &str,
    ) -> Result<Url> {
        let mut url = self.server_root_url.join(&format!(
            "{scope_identifier}/_apis/distributedtask/hubs/{hub_name}/plans/{plan_id}/timelines/{timeline_id}/{tail}"
        ))?;
        url.query_pairs_mut()
            .append_pair("api-version", API_VERSION);
        Ok(url)
    }
}
