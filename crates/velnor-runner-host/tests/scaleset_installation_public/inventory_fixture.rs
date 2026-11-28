// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use serde::Serialize;

use super::hex_sha256;
use crate::{SOURCE_SHA, TestResult};

#[derive(Clone, Serialize)]
pub(crate) struct TestInventoryContext {
    pub(super) event: &'static str,
    pub(super) branch_filters: Vec<&'static str>,
    pub(super) tag_filters: Vec<&'static str>,
    pub(super) pull_request_base_branches: Vec<&'static str>,
}

#[derive(Serialize)]
pub(super) struct TestInventoryJob {
    pub(super) root_path: String,
    pub(super) workflow_path: String,
    pub(super) job_key: String,
    role: &'static str,
    pub(super) contexts: Vec<TestInventoryContext>,
    permissions_declared: bool,
    permissions: std::collections::BTreeMap<&'static str, &'static str>,
    secrets: Vec<&'static str>,
    environments: Vec<&'static str>,
    condition_sha256: Option<&'static str>,
    concurrency: Option<&'static str>,
    pub(super) needs: Vec<String>,
}

#[derive(Serialize)]
pub(super) struct TestInventoryWorkflow {
    pub(super) path: String,
    content_sha256: String,
    calls: Vec<&'static str>,
    pub(super) jobs: Vec<TestInventoryJob>,
}

#[derive(Serialize)]
struct TestInventoryContent<'a> {
    schema_version: u32,
    generator_source_sha: &'a str,
    workflows: &'a [TestInventoryWorkflow],
}

#[derive(Serialize)]
struct TestInventoryEnvelope<'a> {
    schema_version: u32,
    generator_source_sha: &'a str,
    inventory_sha256: String,
    workflows: &'a [TestInventoryWorkflow],
}

pub(super) fn inventory_json_for_contexts(contexts: &[TestInventoryContext]) -> TestResult<String> {
    inventory_json_of(contexts, &BTreeMap::from([("contents", "read")]), 1, 1)
}

pub(super) fn inventory_json_of(
    contexts: &[TestInventoryContext],
    permissions: &BTreeMap<&'static str, &'static str>,
    count: usize,
    job_total: usize,
) -> TestResult<String> {
    inventory_json_from(&workflows_of(contexts, permissions, count, job_total))
}

pub(super) fn workflows_of(
    contexts: &[TestInventoryContext],
    permissions: &BTreeMap<&'static str, &'static str>,
    count: usize,
    job_total: usize,
) -> Vec<TestInventoryWorkflow> {
    (0..count)
        .map(|index| {
            let path = if index == 0 {
                ".github/workflows/ci.yml".to_owned()
            } else {
                format!(".github/workflows/w{index}.yml")
            };
            TestInventoryWorkflow {
                path: path.clone(),
                content_sha256: "a".repeat(64),
                calls: Vec::new(),
                jobs: (index..job_total)
                    .step_by(count)
                    .map(|job| TestInventoryJob {
                        root_path: path.clone(),
                        workflow_path: path.clone(),
                        job_key: if job == 0 {
                            "build".to_owned()
                        } else {
                            format!("build{job:04}")
                        },
                        role: "verification",
                        contexts: contexts.to_vec(),
                        permissions_declared: true,
                        permissions: permissions.clone(),
                        secrets: Vec::new(),
                        environments: Vec::new(),
                        condition_sha256: None,
                        concurrency: None,
                        needs: Vec::new(),
                    })
                    .collect(),
            }
        })
        .collect()
}

pub(super) fn inventory_json_from(workflows: &[TestInventoryWorkflow]) -> TestResult<String> {
    let content = TestInventoryContent {
        schema_version: 1,
        generator_source_sha: SOURCE_SHA,
        workflows,
    };
    let inventory_sha256 = hex_sha256(&serde_json::to_vec(&content)?);
    Ok(serde_json::to_string(&TestInventoryEnvelope {
        schema_version: 1,
        generator_source_sha: SOURCE_SHA,
        inventory_sha256,
        workflows,
    })?)
}
