// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use serde::Serialize;
use serde_json::Value;
use velnor_model::RepoPath;
use velnor_rust::sha256_hex;

use super::JobAuthorityError;

const SCHEMA_VERSION: u32 = 1;

#[derive(Serialize)]
pub(super) struct Inventory {
    schema_version: u32,
    generator_source_sha: String,
    #[serde(rename = "inventory_sha256")]
    sha256: String,
    workflows: Vec<Workflow>,
}

#[derive(Serialize)]
struct Content<'a> {
    schema_version: u32,
    generator_source_sha: &'a str,
    workflows: &'a [Workflow],
}

#[derive(Serialize)]
pub(super) struct Workflow {
    path: String,
    content_sha256: String,
    calls: Vec<Value>,
    jobs: Vec<Job>,
}

#[derive(Serialize)]
pub(super) struct Job {
    root_path: String,
    workflow_path: String,
    #[serde(rename = "job_key")]
    key: String,
    role: &'static str,
    contexts: Vec<Context>,
    permissions_declared: bool,
    permissions: BTreeMap<String, String>,
    secrets: Vec<String>,
    environments: Vec<String>,
    condition_sha256: Option<String>,
    concurrency: Option<Value>,
    needs: Vec<String>,
}

#[derive(Serialize, Clone)]
pub(super) struct Context {
    event: &'static str,
    branch_filters: Vec<String>,
    tag_filters: Vec<String>,
    pull_request_base_branches: Vec<String>,
}

impl Context {
    pub(super) fn new(
        event: &'static str,
        branch_filters: Vec<String>,
        pull_request_base_branches: Vec<String>,
    ) -> Self {
        Self {
            event,
            branch_filters,
            tag_filters: Vec::new(),
            pull_request_base_branches,
        }
    }
}

impl Job {
    pub(super) fn verification(
        path: &RepoPath,
        key: &str,
        contexts: &[Context],
        permissions: BTreeMap<String, String>,
        needs: Vec<String>,
    ) -> Self {
        Self {
            root_path: path.to_string(),
            workflow_path: path.to_string(),
            key: key.to_owned(),
            role: "verification",
            contexts: contexts.to_vec(),
            permissions_declared: true,
            permissions,
            secrets: Vec::new(),
            environments: Vec::new(),
            condition_sha256: None,
            concurrency: None,
            needs,
        }
    }
}

impl Workflow {
    pub(super) fn new(path: &RepoPath, text: &str, jobs: Vec<Job>) -> Self {
        Self {
            path: path.to_string(),
            content_sha256: sha256_hex(text.as_bytes()),
            calls: Vec::new(),
            jobs,
        }
    }
}

impl Inventory {
    pub(super) fn new(
        generator_source_sha: String,
        workflows: Vec<Workflow>,
    ) -> Result<Self, JobAuthorityError> {
        let content = Content {
            schema_version: SCHEMA_VERSION,
            generator_source_sha: &generator_source_sha,
            workflows: &workflows,
        };
        let bytes = serde_json::to_vec(&content)
            .map_err(|error| JobAuthorityError::Render(error.to_string()))?;
        Ok(Self {
            schema_version: SCHEMA_VERSION,
            sha256: sha256_hex(&bytes),
            generator_source_sha,
            workflows,
        })
    }

    pub(super) fn render(&self) -> Result<String, JobAuthorityError> {
        serde_json::to_string_pretty(self)
            .map(|text| text + "\n")
            .map_err(|error| JobAuthorityError::Render(error.to_string()))
    }
}
