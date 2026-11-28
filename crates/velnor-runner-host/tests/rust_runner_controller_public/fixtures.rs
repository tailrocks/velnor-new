// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;
use std::fmt::Write;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde::Serialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use velnor_runner_host::{ConnectionId, HostConfig, build_job_authority_policy_json};

use super::harness::CAP_GIB;

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

pub(super) fn merge_group_offer(
    request_id: u64,
    run_id: u64,
    reference: &str,
    branch: &str,
) -> Value {
    json!({
        "request_id": request_id,
        "event": "merge_group",
        "run_id": run_id,
        "ref": reference,
        "branch": branch,
        "sha": sha(run_id),
        "workflow_content": WORKFLOW,
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

pub(super) fn with_acquire_returns(mut offer: Value, shape: &str) -> Value {
    offer["acquire_returns"] = json!(shape);
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

fn inventory(merge_group: bool) -> TestResult<String> {
    let contexts = vec![
        Context {
            event: "push",
            branch_filters: vec!["main"],
            tag_filters: Vec::new(),
            pull_request_base_branches: Vec::new(),
        },
        if merge_group {
            Context {
                event: "merge_group",
                branch_filters: vec!["main"],
                tag_filters: Vec::new(),
                pull_request_base_branches: Vec::new(),
            }
        } else {
            Context {
                event: "pull_request",
                branch_filters: Vec::new(),
                tag_filters: Vec::new(),
                pull_request_base_branches: vec!["main"],
            }
        },
    ];
    let workflows = vec![InventoryWorkflow {
        path: ".github/workflows/ci.yml",
        content_sha256: hex_sha256(WORKFLOW.as_bytes()),
        calls: Vec::new(),
        jobs: vec![InventoryJob {
            root_path: ".github/workflows/ci.yml",
            workflow_path: ".github/workflows/ci.yml",
            job_key: "build",
            role: "verification",
            contexts,
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

const BRANCH_POLICY: &str = "mode = \"branch_restricted\"\nbranches = [\"main\"]\ntag_prefixes = []\nevents = [\"push\", \"pull_request\"]\npull_request_rules = [{ base_repository_id = 42, base_branch = \"main\", head_repository_id = 42, head_branch = \"feature\" }, { base_repository_id = 42, base_branch = \"main\", head_repository_id = 43, head_branch = \"feature\" }]";
const MERGE_GROUP_POLICY: &str = "mode = \"repository\"\nbranches = []\ntag_prefixes = []\nevents = [\"push\", \"merge_group\"]\npull_request_rules = []";
const REPOSITORY_PULL_REQUEST_POLICY: &str = "mode = \"repository\"\nbranches = []\ntag_prefixes = []\nevents = [\"push\", \"pull_request\"]\npull_request_rules = []";
pub(super) const KEYCHAIN_REFERENCE: &str = "keychain:com.tailrocks.velnor.host/lab";
pub(super) const HOST_TOML_LIMIT: usize = 1_048_576;

pub(super) fn host_toml(root: &Path, max_jobs: u32) -> TestResult<String> {
    host_toml_for(root, max_jobs, KEYCHAIN_REFERENCE)
}

pub(super) fn host_toml_for(
    root: &Path,
    max_jobs: u32,
    credential_ref: &str,
) -> TestResult<String> {
    host_toml_with(root, max_jobs, credential_ref, "", BRANCH_POLICY)
}

pub(super) fn organization_host_toml(root: &Path, max_jobs: u32) -> TestResult<String> {
    host_toml_with(
        root,
        max_jobs,
        KEYCHAIN_REFERENCE,
        "organization = \"tailrocks\"\n",
        BRANCH_POLICY,
    )
}

pub(super) fn merge_group_host_toml(root: &Path) -> TestResult<String> {
    host_toml_with(root, 1, KEYCHAIN_REFERENCE, "", MERGE_GROUP_POLICY)
}

pub(super) fn repository_pull_request_host_toml(root: &Path) -> TestResult<String> {
    host_toml_with(
        root,
        1,
        KEYCHAIN_REFERENCE,
        "",
        REPOSITORY_PULL_REQUEST_POLICY,
    )
}

pub(super) fn with_head_ref(mut offer: Value, branch: &str) -> Value {
    offer["branch"] = json!(branch);
    offer["pr"]["head_ref"] = json!(branch);
    offer
}

pub(super) fn padded(text: &str, bytes: usize) -> String {
    let fill = bytes.saturating_sub(text.len() + 4);
    format!("{text}\n# {}\n", "x".repeat(fill))
}

fn host_toml_with(
    root: &Path,
    max_jobs: u32,
    credential_ref: &str,
    connection_lines: &str,
    source_policy: &str,
) -> TestResult<String> {
    let base = format!(
        "schema = 2\n\
         [[connections]]\n\
         connection_id = \"lab\"\n\
         repository_id = 42\n\
         repository = \"tailrocks/velnor-lab\"\n\
         credential_ref = \"{credential_ref}\"\n\
         {connection_lines}\
         [connections.scale_set]\n\
         name = \"velnor-mac\"\n\
         labels = [\"velnor\", \"velnor-mac\"]\n\
         [connections.source_policy]\n\
         {policy}\n\
         [host]\n\
         launch_mode = \"rust-runner\"\n\
         max_jobs = {max_jobs}\n\
         [host.resources]\n\
         runner_cpu_millicores = 2000\n\
         runner_memory_bytes = 4294967296\n\
         dind_cpu_millicores = 1000\n\
         dind_memory_bytes = 2147483648\n\
         operational_memory_reserve_bytes = 1073741824\n\
         [host.machine_disk]\n\
         store_budget_gib = {budget}\n\
         [docker]\n\
         context = \"orbstack\"\n\
         platform = \"linux/arm64\"\n\
         endpoint = \"unix://{}/no-such-docker.sock\"\n",
        root.display(),
        policy = source_policy,
        budget = CAP_GIB - 72 * u64::from(max_jobs),
    );
    let config = HostConfig::parse_draft(&base)?;
    let id = ConnectionId::parse("lab")?;
    let connection = config.connection(&id).ok_or("connection lab")?;
    let policy = build_job_authority_policy_json(
        connection,
        &inventory(source_policy.contains("merge_group"))?,
        SOURCE_SHA,
        &"d".repeat(64),
        1,
    )?;
    Ok(config
        .with_job_authority_policy_json(&id, &policy)?
        .to_toml()?)
}

pub(super) fn official_host_toml(root: &Path, credential_ref: &str) -> String {
    format!(
        "schema = 2\n\
         [[connections]]\n\
         connection_id = \"lab\"\n\
         repository_id = 42\n\
         repository = \"tailrocks/velnor-lab\"\n\
         credential_ref = \"{credential_ref}\"\n\
         [connections.scale_set]\n\
         name = \"ubuntu-26.04-scale-set\"\n\
         labels = [\"velnor\", \"ubuntu-26.04-scale-set\"]\n\
         [connections.source_policy]\n\
         mode = \"branch_restricted\"\n\
         branches = [\"main\"]\n\
         tag_prefixes = []\n\
         events = [\"push\"]\n\
         pull_request_rules = []\n\
         [host]\n\
         launch_mode = \"official-runner\"\n\
         max_jobs = 1\n\
         [host.resources]\n\
         runner_cpu_millicores = 2000\n\
         runner_memory_bytes = 4294967296\n\
         dind_cpu_millicores = 1000\n\
         dind_memory_bytes = 2147483648\n\
         operational_memory_reserve_bytes = 1073741824\n\
         [docker]\n\
         context = \"orbstack\"\n\
         platform = \"linux/amd64\"\n\
         endpoint = \"unix://{}/no-such-docker.sock\"\n",
        root.display()
    )
}

pub(super) fn with_head_repository(mut offer: Value, repository_id: u64) -> Value {
    offer["head_repository_id"] = json!(repository_id);
    offer
}

pub(super) fn with_run_repository(mut offer: Value, repository_id: u64) -> Value {
    offer["run_repository_id"] = json!(repository_id);
    offer
}

pub(super) fn with_jit_failures(mut offer: Value, count: u64) -> Value {
    offer["jit_failures"] = json!(count);
    offer
}

pub(super) fn with_jit_identity(mut offer: Value, field: &str) -> Value {
    offer["jit_identity"] = json!(field);
    offer
}

pub(super) fn bun() -> TestResult<PathBuf> {
    let output = Command::new("bun")
        .args(["--print", "process.execPath"])
        .output()
        .map_err(|_| "the controller tests need Bun on PATH")?;
    let path = String::from_utf8(output.stdout)?.trim().to_owned();
    if !output.status.success() || !Path::new(&path).is_absolute() {
        return Err("bun did not report its executable".into());
    }
    Ok(PathBuf::from(path))
}

pub(super) fn curl_wrapper(bun: &Path) -> String {
    format!(
        "#!/bin/sh\n\
         input=\"$VELNOR_FAKE_DIR/seen/.curl.$$\"\n\
         cat > \"$input\"\n\
         {bun} \"$VELNOR_FAKE_DIR/fake_curl.ts\" < \"$input\"\n\
         status=$?\n\
         rm -f \"$input\"\n\
         exit \"$status\"\n",
        bun = bun.display()
    )
}
