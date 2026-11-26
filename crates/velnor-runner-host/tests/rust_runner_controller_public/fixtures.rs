// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;
use std::fmt::Write;
use std::path::Path;

use serde::Serialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use velnor_runner_host::{ConnectionId, HostConfig, build_job_authority_policy_json};

pub(super) const WORKFLOW: &str = "name: ci\non: [push, pull_request]\njobs:\n  build:\n    runs-on: velnor-mac\n    steps:\n      - run: echo ok\n";
pub(super) const OTHER_WORKFLOW: &str = "name: ci\non: [push]\njobs:\n  build:\n    runs-on: velnor-mac\n    steps:\n      - run: echo other\n";
const SOURCE_SHA: &str = "1111111111111111111111111111111111111111";

pub(super) type TestResult<T> = Result<T, Box<dyn Error>>;

pub(super) fn hex_sha256(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .fold(String::new(), |mut text, byte| {
            let _ = write!(text, "{byte:02x}");
            text
        })
}

pub(super) fn sha(number: u64) -> String {
    format!("{number:040x}")
}

pub(super) fn push_offer(request_id: u64, run_id: u64, branch: &str, content: &str) -> Value {
    json!({
        "request_id": request_id,
        "event": "push",
        "run_id": run_id,
        "ref": format!("refs/heads/{branch}"),
        "branch": branch,
        "sha": sha(run_id),
        "workflow_content": content,
    })
}

pub(super) fn pull_offer(
    request_id: u64,
    run_id: u64,
    number: u64,
    merge_parents: Option<&[String; 2]>,
) -> Value {
    let base = sha(run_id * 10 + 1);
    let head = sha(run_id * 10 + 2);
    json!({
        "request_id": request_id,
        "event": "pull_request",
        "run_id": run_id,
        "ref": format!("refs/pull/{number}/merge"),
        "branch": "feature",
        "sha": sha(run_id),
        "workflow_content": WORKFLOW,
        "pr": {
            "number": number,
            "base_sha": base,
            "head_sha": head,
            "base_ref": "main",
            "head_ref": "feature",
            "merge_parents": merge_parents,
        },
    })
}

pub(super) fn with_acquire_none(mut offer: Value) -> Value {
    offer["acquire_returns"] = json!("none");
    offer
}

#[derive(Serialize)]
struct Context {
    event: &'static str,
    branch_filters: Vec<&'static str>,
    tag_filters: Vec<&'static str>,
    pull_request_base_branches: Vec<&'static str>,
}

#[derive(Serialize)]
struct InventoryJob {
    root_path: &'static str,
    workflow_path: &'static str,
    job_key: &'static str,
    role: &'static str,
    contexts: Vec<Context>,
    permissions_declared: bool,
    permissions: std::collections::BTreeMap<&'static str, &'static str>,
    secrets: Vec<&'static str>,
    environments: Vec<&'static str>,
    condition_sha256: Option<&'static str>,
    concurrency: Option<&'static str>,
    needs: Vec<&'static str>,
}

#[derive(Serialize)]
struct InventoryWorkflow {
    path: &'static str,
    content_sha256: String,
    calls: Vec<&'static str>,
    jobs: Vec<InventoryJob>,
}

#[derive(Serialize)]
struct InventoryContent<'a> {
    schema_version: u32,
    generator_source_sha: &'a str,
    workflows: &'a [InventoryWorkflow],
}

#[derive(Serialize)]
struct InventoryEnvelope<'a> {
    schema_version: u32,
    generator_source_sha: &'a str,
    inventory_sha256: String,
    workflows: &'a [InventoryWorkflow],
}

fn inventory() -> TestResult<String> {
    let workflows = vec![InventoryWorkflow {
        path: ".github/workflows/ci.yml",
        content_sha256: hex_sha256(WORKFLOW.as_bytes()),
        calls: Vec::new(),
        jobs: vec![InventoryJob {
            root_path: ".github/workflows/ci.yml",
            workflow_path: ".github/workflows/ci.yml",
            job_key: "build",
            role: "verification",
            contexts: vec![
                Context {
                    event: "push",
                    branch_filters: vec!["main"],
                    tag_filters: Vec::new(),
                    pull_request_base_branches: Vec::new(),
                },
                Context {
                    event: "pull_request",
                    branch_filters: Vec::new(),
                    tag_filters: Vec::new(),
                    pull_request_base_branches: vec!["main"],
                },
            ],
            permissions_declared: true,
            permissions: std::collections::BTreeMap::from([("contents", "read")]),
            secrets: Vec::new(),
            environments: Vec::new(),
            condition_sha256: None,
            concurrency: None,
            needs: Vec::new(),
        }],
    }];
    let content = InventoryContent {
        schema_version: 1,
        generator_source_sha: SOURCE_SHA,
        workflows: &workflows,
    };
    let inventory_sha256 = hex_sha256(&serde_json::to_vec(&content)?);
    Ok(serde_json::to_string(&InventoryEnvelope {
        schema_version: 1,
        generator_source_sha: SOURCE_SHA,
        inventory_sha256,
        workflows: &workflows,
    })?)
}

pub(super) fn host_toml(root: &Path, max_jobs: u32) -> TestResult<String> {
    let base = format!(
        "schema = 2\n\
         [[connections]]\n\
         connection_id = \"lab\"\n\
         repository_id = 42\n\
         repository = \"tailrocks/velnor-lab\"\n\
         credential_ref = \"keychain:com.tailrocks.velnor.host/lab\"\n\
         [connections.scale_set]\n\
         name = \"velnor-mac\"\n\
         labels = [\"velnor\", \"velnor-mac\"]\n\
         [connections.source_policy]\n\
         mode = \"branch_restricted\"\n\
         branches = [\"main\"]\n\
         tag_prefixes = []\n\
         events = [\"push\", \"pull_request\"]\n\
         pull_request_rules = [{{ base_repository_id = 42, base_branch = \"main\", head_repository_id = 42, head_branch = \"feature\" }}]\n\
         [host]\n\
         launch_mode = \"rust-runner\"\n\
         max_jobs = {max_jobs}\n\
         [host.resources]\n\
         runner_cpu_millicores = 2000\n\
         runner_memory_bytes = 4294967296\n\
         dind_cpu_millicores = 1000\n\
         dind_memory_bytes = 2147483648\n\
         operational_memory_reserve_bytes = 1073741824\n\
         [docker]\n\
         context = \"orbstack\"\n\
         platform = \"linux/arm64\"\n\
         endpoint = \"unix://{}/no-such-docker.sock\"\n",
        root.display()
    );
    let config = HostConfig::parse_draft(&base)?;
    let id = ConnectionId::parse("lab")?;
    let connection = config.connection(&id).ok_or("connection lab")?;
    let policy =
        build_job_authority_policy_json(connection, &inventory()?, SOURCE_SHA, &"d".repeat(64), 1)?;
    Ok(config
        .with_job_authority_policy_json(&id, &policy)?
        .to_toml()?)
}
