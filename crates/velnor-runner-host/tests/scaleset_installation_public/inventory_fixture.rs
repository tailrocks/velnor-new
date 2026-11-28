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
struct TestInventoryJob {
    root_path: &'static str,
    workflow_path: &'static str,
    job_key: &'static str,
    role: &'static str,
    contexts: Vec<TestInventoryContext>,
    permissions_declared: bool,
    permissions: std::collections::BTreeMap<&'static str, &'static str>,
    secrets: Vec<&'static str>,
    environments: Vec<&'static str>,
    condition_sha256: Option<&'static str>,
    concurrency: Option<&'static str>,
    needs: Vec<&'static str>,
}

#[derive(Serialize)]
struct TestInventoryWorkflow {
    path: &'static str,
    content_sha256: String,
    calls: Vec<&'static str>,
    jobs: Vec<TestInventoryJob>,
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

pub(super) fn inventory_json_for_contexts(
    contexts: Vec<TestInventoryContext>,
) -> TestResult<String> {
    inventory_json(contexts, BTreeMap::from([("contents", "read")]))
}

pub(super) fn inventory_json(
    contexts: Vec<TestInventoryContext>,
    permissions: BTreeMap<&'static str, &'static str>,
) -> TestResult<String> {
    let workflows = vec![TestInventoryWorkflow {
        path: ".github/workflows/ci.yml",
        content_sha256: "a".repeat(64),
        calls: Vec::new(),
        jobs: vec![TestInventoryJob {
            root_path: ".github/workflows/ci.yml",
            workflow_path: ".github/workflows/ci.yml",
            job_key: "build",
            role: "verification",
            contexts,
            permissions_declared: true,
            permissions,
            secrets: Vec::new(),
            environments: Vec::new(),
            condition_sha256: None,
            concurrency: None,
            needs: Vec::new(),
        }],
    }];
    let content = TestInventoryContent {
        schema_version: 1,
        generator_source_sha: SOURCE_SHA,
        workflows: &workflows,
    };
    let inventory_sha256 = hex_sha256(&serde_json::to_vec(&content)?);
    Ok(serde_json::to_string(&TestInventoryEnvelope {
        schema_version: 1,
        generator_source_sha: SOURCE_SHA,
        inventory_sha256,
        workflows: &workflows,
    })?)
}
