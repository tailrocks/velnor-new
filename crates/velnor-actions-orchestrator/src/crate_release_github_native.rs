// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::{collections::BTreeMap, time::Duration};

use serde_json::Value;

use super::{GitHubAdmissionRead, require};
use crate::{OrchestratorError, internal::internal};

pub struct NativeGitHubAdmissionRead {
    pub(crate) reader: crate::crate_release_http::GitHubAdmissionHttp,
}

impl std::fmt::Debug for NativeGitHubAdmissionRead {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("NativeGitHubAdmissionRead")
            .finish_non_exhaustive()
    }
}

impl NativeGitHubAdmissionRead {
    pub fn new(token: String) -> Result<Self, OrchestratorError> {
        require(!token.is_empty(), "github_token_missing")?;
        let timeout = Duration::from_secs(180);
        Ok(Self {
            reader: crate::crate_release_http::GitHubAdmissionHttp::new(token, timeout)?,
        })
    }
}

impl crate::crate_release_controller::CrateReleaseRepairWrite for NativeGitHubAdmissionRead {
    fn create_tag(
        &self,
        repository: &str,
        tag: &str,
        source_sha: &str,
    ) -> Result<(), OrchestratorError> {
        self.reader.create_tag(repository, tag, source_sha)
    }
    fn create_release(
        &self,
        repository: &str,
        tag: &str,
        source_sha: &str,
        version: &str,
    ) -> Result<(), OrchestratorError> {
        self.reader
            .create_release(repository, tag, source_sha, version)
    }
}

impl GitHubAdmissionRead for NativeGitHubAdmissionRead {
    fn get(&self, route: &str) -> Result<Value, OrchestratorError> {
        self.reader.get(route)
    }
    fn get_artifact_page(
        &self,
        route: &str,
        limit: u64,
    ) -> Result<(Value, u64), OrchestratorError> {
        self.reader.get_artifact_page(route, limit)
    }
    fn get_optional(&self, route: &str) -> Result<Option<Value>, OrchestratorError> {
        self.reader.get_optional(route)
    }
}

impl crate::crate_release_qualification_witness::WitnessRead for NativeGitHubAdmissionRead {
    fn witness_job_log(&self, repository: &str, job_id: u64) -> Result<Vec<u8>, OrchestratorError> {
        self.reader.qualification_job_log(repository, job_id)
    }

    fn artifact_bytes(
        &self,
        repository: &str,
        artifact_id: u64,
        expected_size: u64,
        expected_sha256: &str,
    ) -> Result<Vec<u8>, OrchestratorError> {
        self.reader.qualification_artifact_bytes(
            repository,
            artifact_id,
            expected_size,
            expected_sha256,
        )
    }
}

impl crate::github_review_feedback::FeedbackRead for NativeGitHubAdmissionRead {
    fn feedback_get(&self, route: &str) -> Result<Value, OrchestratorError> {
        self.reader.get(route)
    }

    fn feedback_graphql(
        &self,
        query: crate::github_review_feedback::FeedbackQuery,
        fields: &[(&str, String)],
    ) -> Result<Value, OrchestratorError> {
        let mut values = BTreeMap::new();
        for (name, value) in fields {
            if !matches!(*name, "owner" | "name" | "number" | "cursor")
                || values.insert(*name, value.as_str()).is_some()
            {
                return Err(internal("qualification_feedback_fields"));
            }
        }
        let owner = values
            .get("owner")
            .ok_or_else(|| internal("qualification_feedback_fields"))?;
        let name = values
            .get("name")
            .ok_or_else(|| internal("qualification_feedback_fields"))?;
        let number = values
            .get("number")
            .and_then(|s| s.parse::<u32>().ok())
            .filter(|n| *n > 0)
            .ok_or_else(|| internal("qualification_feedback_fields"))?;
        let repository = format!("{owner}/{name}");
        let cursor = values.get("cursor").copied();
        match query {
            crate::github_review_feedback::FeedbackQuery::Threads => self
                .reader
                .qualification_review_threads(&repository, number, cursor),
            crate::github_review_feedback::FeedbackQuery::Opinions => self
                .reader
                .qualification_opinionated_reviews(&repository, number, cursor),
        }
    }
}

impl crate::tap_merge::review_completion::provider::ProviderRead for NativeGitHubAdmissionRead {
    fn provider_app(
        &self,
        app: crate::tap_merge::review_completion::provider::ProviderApp,
    ) -> Result<Value, OrchestratorError> {
        self.reader.get(app.route())
    }
    fn provider_graphql(
        &self,
        query: crate::tap_merge::review_completion::provider::ProviderQuery,
        fields: &[(&str, String)],
    ) -> Result<Value, OrchestratorError> {
        self.reader.qualification_provider_graphql(query, fields)
    }
    fn provider_check_runs(
        &self,
        repository: &str,
        head: &str,
    ) -> Result<Vec<Value>, OrchestratorError> {
        crate::crate_release_qualification_checks::complete_check_runs(self, repository, head)
    }
}
