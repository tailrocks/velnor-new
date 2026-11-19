// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

//! The world a `resolve-qualification` case runs in: a git repository with fixed commits, the
//! dispatch inputs and the request `write-request` wrote, and a GitHub API holding the
//! predecessor runs with their receipt artifacts. `record_legacy` runs the legacy engine in this
//! world; the tests run `velnor-ci` in the same world.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;

use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use crate::support::fresh_dir;

pub(crate) const REPO: &str = "tailrocks/velnor-new";
pub(crate) const WORKFLOW: &str = ".github/workflows/ci.yml";
pub(crate) const ARTIFACT: &str = "velnor-qualification-cache-receipt-v2";
pub(crate) const MEMBER: &str = "qualification-cache-receipt.json";
const CURRENT_RUN: u64 = 2001;
static TEMPLATE: OnceLock<(PathBuf, Shas)> = OnceLock::new();

pub(crate) fn data_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/it/data/resolve_qualification")
}

#[derive(Clone)]
pub(crate) struct Shas {
    pub(crate) base: String,
    pub(crate) source: String,
    pub(crate) side: String,
    pub(crate) many: String,
}

pub(crate) fn make_repo(dir: &Path) -> Shas {
    let status = Command::new("sh")
        .arg(data_dir().join("repo.sh"))
        .arg(dir)
        .output()
        .unwrap_or_else(|e| unreachable!("{e}"));
    assert!(status.status.success(), "repo.sh: {status:?}");
    let rev = |name: &str| {
        let out = Command::new("git")
            .arg("-C")
            .arg(dir)
            .args(["rev-parse", name])
            .output()
            .unwrap_or_else(|e| unreachable!("{e}"));
        String::from_utf8_lossy(&out.stdout).trim().to_owned()
    };
    Shas {
        base: rev("main~1"),
        source: rev("main"),
        side: rev("side"),
        many: rev("many"),
    }
}

/// The repository `repo.sh` builds, made once and copied for every case.
fn template() -> &'static (PathBuf, Shas) {
    TEMPLATE.get_or_init(|| {
        let dir = fresh_dir("velnor-ci-resolve", "template").join("repo");
        let shas = make_repo(&dir);
        (dir, shas)
    })
}

pub(crate) fn copy_repo(dest: &Path) {
    let copied = Command::new("cp")
        .arg("-R")
        .arg(format!("{}/.", template().0.display()))
        .arg(dest)
        .status()
        .unwrap_or_else(|e| unreachable!("{e}"));
    assert!(copied.success(), "cp -R of the template repository");
}

pub(crate) fn digest_b3(bytes: &[u8]) -> String {
    format!("b3-{}", blake3::hash(bytes).to_hex())
}

pub(crate) fn sha256(bytes: &[u8]) -> String {
    let mut text = String::from("sha256:");
    for byte in Sha256::digest(bytes) {
        write!(text, "{byte:02x}").unwrap_or_else(|e| unreachable!("{e}"));
    }
    text
}

fn zip_of(name: &str, bytes: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    {
        let mut zip = zip::ZipWriter::new(std::io::Cursor::new(&mut out));
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Stored);
        zip.start_file(name, options)
            .unwrap_or_else(|e| unreachable!("{e}"));
        zip.write_all(bytes).unwrap_or_else(|e| unreachable!("{e}"));
        zip.finish().unwrap_or_else(|e| unreachable!("{e}"));
    }
    out
}

pub(crate) fn zip_with(name: &str, bytes: &[u8]) -> Vec<u8> {
    zip_of(name, bytes)
}

/// What is odd about where the op runs, besides the request, the API and the commits.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Quirk {
    RequestIsASymlink,
    AdmissionAlreadyStaged,
    VelnorDirectoryIsASymlink,
    NotARepository,
}

/// One run of the lineage, as the API and its receipt artifact describe it.
pub(crate) struct Node {
    pub(crate) run_id: u64,
    pub(crate) receipt: Value,
    pub(crate) producer: Value,
    pub(crate) run: Value,
    pub(crate) artifact: Value,
    pub(crate) zip_override: Option<Vec<u8>>,
    pub(crate) zip: Vec<u8>,
}

impl Node {
    fn artifact_id(&self) -> u64 {
        5000 + self.run_id
    }

    fn run_ref(&self) -> Value {
        json!({"run_id": self.run_id, "run_attempt": 1})
    }
}

type Edit = Box<dyn Fn(&mut Node)>;

/// Everything outside the repository a case reads, and the ways a case can differ.
pub(crate) struct World {
    pub(crate) shas: Shas,
    pub(crate) request: Value,
    pub(crate) payload: Value,
    pub(crate) env: BTreeMap<String, String>,
    pub(crate) nodes: Vec<Node>,
    pub(crate) api: BTreeMap<String, Vec<u8>>,
    pub(crate) request_bytes: Option<Vec<u8>>,
    pub(crate) payload_bytes: Option<Vec<u8>>,
    pub(crate) request_name: Option<String>,
    pub(crate) unset: Vec<&'static str>,
    pub(crate) quirk: Option<Quirk>,
    edits: Vec<(usize, Edit)>,
    finished: bool,
}

fn lane(key: &str) -> Value {
    let digest = digest_b3(key.as_bytes());
    json!({
        "matrix_key": key, "stack_id": "rust", "task_id": "stack/rust/x/build/default",
        "completed_task_ids": ["stack/rust/x/build/default"],
        "closure_digest": digest, "useful_state_digest": digest,
        "layers": [{
            "layer": "mbx_objects", "active": true, "identity_digest": digest,
            "runtime_identity": {
                "platform": {"os": "linux", "arch": "x86_64", "runs_on": "ubuntu", "image_os": "ubuntu26", "target": "x86_64-unknown-linux-gnu"},
                "toolchain_id": "t", "cache_format_id": "c", "abi_id": "a", "driver_id": "d"
            },
            "state_digest": digest,
            "restore": {"requested_key": "k", "matched_key": null, "matched_cache": {"status": "absent"}, "result": "miss"},
            "save": {
                "requested_key": "k", "action": "succeeded",
                "before": {"status": "not_queried"},
                "after": {"status": "found", "entry": {"id": 7, "key": "k", "git_ref": "refs/heads/main", "size_bytes": 9}}
            }
        }]
    })
}

impl World {
    /// A `phase` dispatch with its predecessors: warm has the cold run, third the warm and cold,
    /// `useful_delta` the third, warm and cold.
    pub(crate) fn new(phase: &str) -> Self {
        let shas = template().1.clone();
        let phases: &[&str] = match phase {
            "warm" => &["cold"],
            "third" => &["warm", "cold"],
            "useful_delta" => &["third", "warm", "cold"],
            _ => &[],
        };
        let nodes: Vec<Node> = phases
            .iter()
            .map(|name| {
                let run_id = match *name {
                    "cold" => 1001,
                    "warm" => 1002,
                    _ => 1003,
                };
                Self::node(name, run_id, &shas.base)
            })
            .collect();
        let source = if phase == "useful_delta" {
            &shas.source
        } else {
            &shas.base
        }
        .clone();
        let mut context = json!({
            "campaign": "camp-1435", "default_branch": "main", "git_ref": "refs/heads/main",
            "phase": phase, "ref_protected": true, "repository": REPO, "run_attempt": 1,
            "run_id": CURRENT_RUN, "source_sha": source,
            "workflow_ref": format!("{REPO}/{WORKFLOW}@refs/heads/main"), "workflow_sha": source,
        });
        let mut inputs = json!({"campaign": "camp-1435", "phase": phase});
        if let Some(first) = nodes.first() {
            context["predecessor"] = first.run_ref();
            inputs["predecessor_run_id"] = json!(first.run_id.to_string());
            inputs["predecessor_run_attempt"] = json!("1");
        }
        let env = [
            ("GITHUB_REPOSITORY", REPO.to_owned()),
            ("GITHUB_REF", "refs/heads/main".to_owned()),
            ("GITHUB_REF_PROTECTED", "true".to_owned()),
            (
                "GITHUB_WORKFLOW_REF",
                format!("{REPO}/{WORKFLOW}@refs/heads/main"),
            ),
            ("GITHUB_WORKFLOW_SHA", source.clone()),
            ("GITHUB_SHA", source.clone()),
            ("GITHUB_RUN_ID", CURRENT_RUN.to_string()),
            ("GITHUB_RUN_ATTEMPT", "1".to_owned()),
        ]
        .into_iter()
        .map(|(key, value)| (key.to_owned(), value))
        .collect();
        Self {
            request: json!({
                "base": null, "event": "qualification", "head": source, "op": "plan-v1",
                "qualification": context, "repository": REPO, "root": ".", "schema": 1
            }),
            payload: json!({
                "ref": "refs/heads/main", "inputs": inputs, "workflow": WORKFLOW,
                "repository": {"full_name": REPO, "default_branch": "main"}
            }),
            shas,
            env,
            nodes,
            api: BTreeMap::new(),
            request_bytes: None,
            payload_bytes: None,
            request_name: None,
            unset: Vec::new(),
            quirk: None,
            edits: Vec::new(),
            finished: false,
        }
    }

    fn node(phase: &str, run_id: u64, source: &str) -> Node {
        let run = json!({"run_id": run_id, "run_attempt": 1});
        let digest = digest_b3(phase.as_bytes());
        Node {
            run_id,
            receipt: json!({
                "schema": 2, "plan_id": format!("plan-r{run_id}-a1"), "run": run,
                "campaign": "camp-1435", "phase": phase, "source_sha": source,
                "configuration_digest": digest, "source_delta": null, "predecessor": null,
                "lanes": [lane("m-0000000000000001"), lane("m-0000000000000002")]
            }),
            producer: json!({
                "repository": REPO, "default_branch": "main", "git_ref": "refs/heads/main",
                "ref_protected": true, "workflow_ref": format!("{REPO}/{WORKFLOW}@refs/heads/main"),
                "workflow_sha": source, "source_sha": source, "run": run
            }),
            run: json!({
                "id": run_id, "run_attempt": 1, "event": "workflow_dispatch",
                "status": "completed", "conclusion": "success", "head_branch": "main",
                "head_sha": source, "path": WORKFLOW, "head_repository": {"full_name": REPO}
            }),
            artifact: Value::Null,
            zip_override: None,
            zip: Vec::new(),
        }
    }

    /// Change node `index` after its link to its predecessor is set and before it is packed.
    pub(crate) fn edit(&mut self, index: usize, change: impl Fn(&mut Node) + 'static) {
        self.edits.push((index, Box::new(change)));
    }

    /// Link, pack and publish the lineage; a second call does nothing.
    pub(crate) fn finish(&mut self) {
        if self.finished {
            return;
        }
        self.finished = true;
        for index in (0..self.nodes.len()).rev() {
            if let Some(older) = self.nodes.get(index + 1) {
                let link = json!({
                    "run": older.run_ref(),
                    "receipt_digest": digest_b3(older.receipt.to_string().as_bytes()),
                    "artifact_id": older.artifact_id(),
                    "artifact_digest": older.artifact["digest"],
                });
                self.nodes[index].receipt["predecessor"] = link;
            }
            for (at, change) in &self.edits {
                if *at == index {
                    change(&mut self.nodes[index]);
                }
            }
            let node = &mut self.nodes[index];
            let document = json!({"schema": 2, "producer": node.producer, "receipt": node.receipt});
            node.zip = node
                .zip_override
                .clone()
                .unwrap_or_else(|| zip_of(MEMBER, document.to_string().as_bytes()));
            node.artifact = json!({
                "id": node.artifact_id(), "name": ARTIFACT, "digest": sha256(&node.zip),
                "size_in_bytes": node.zip.len(), "expired": false,
                "workflow_run": {
                    "id": node.run_id, "head_branch": "main", "head_sha": node.run["head_sha"]
                }
            });
        }
        self.publish();
    }

    fn publish(&mut self) {
        let json_of = |value: &Value| value.to_string().into_bytes();
        let mut api = BTreeMap::new();
        api.insert(
            format!("repos/{REPO}"),
            json_of(&json!({"full_name": REPO, "default_branch": "main"})),
        );
        api.insert(
            format!("repos/{REPO}/branches/main"),
            json_of(&json!({"protected": true})),
        );
        for node in &self.nodes {
            let id = node.run_id;
            let artifact_id = node.artifact_id();
            api.insert(
                format!("repos/{REPO}/actions/runs/{id}/attempts/1"),
                json_of(&node.run),
            );
            api.insert(
                format!("repos/{REPO}/actions/runs/{id}/artifacts?per_page=100"),
                json_of(&json!({"total_count": 1, "artifacts": [node.artifact]})),
            );
            api.insert(
                format!("repos/{REPO}/actions/artifacts/{artifact_id}"),
                json_of(&node.artifact),
            );
            api.insert(
                format!("repos/{REPO}/actions/artifacts/{artifact_id}/zip"),
                node.zip.clone(),
            );
        }
        self.api = api;
    }

    /// Replace one API answer with the result of `change` applied to its JSON.
    pub(crate) fn api_edit(&mut self, route: &str, change: impl FnOnce(&mut Value)) {
        self.finish();
        let body = self
            .api
            .get(route)
            .unwrap_or_else(|| unreachable!("no route {route}"));
        let mut value: Value = serde_json::from_slice(body).unwrap_or_else(|e| unreachable!("{e}"));
        change(&mut value);
        self.api
            .insert(route.to_owned(), value.to_string().into_bytes());
    }

    pub(crate) fn api_raw(&mut self, route: &str, body: &[u8]) {
        self.finish();
        self.api.insert(route.to_owned(), body.to_vec());
    }

    pub(crate) fn api_drop(&mut self, route: &str) {
        self.finish();
        self.api.remove(route);
    }

    pub(crate) fn dispatch(&mut self, change: impl FnOnce(&mut Value)) {
        change(&mut self.request["qualification"]);
    }

    /// The commit the dispatch runs at: the request, its context and the runner's variables.
    pub(crate) fn dispatch_source(&mut self, sha: &str) {
        self.request["head"] = json!(sha);
        self.request["qualification"]["source_sha"] = json!(sha);
        self.request["qualification"]["workflow_sha"] = json!(sha);
        self.env.insert("GITHUB_SHA".to_owned(), sha.to_owned());
        self.env
            .insert("GITHUB_WORKFLOW_SHA".to_owned(), sha.to_owned());
    }

    /// The commit every predecessor run was at.
    pub(crate) fn chain_source(&mut self, sha: &str) {
        for node in &mut self.nodes {
            node.receipt["source_sha"] = json!(sha);
            node.producer["workflow_sha"] = json!(sha);
            node.producer["source_sha"] = json!(sha);
            node.run["head_sha"] = json!(sha);
        }
    }
}

pub(crate) fn run_route(id: u64) -> String {
    format!("repos/{REPO}/actions/runs/{id}/attempts/1")
}

pub(crate) fn list_route(id: u64) -> String {
    format!("repos/{REPO}/actions/runs/{id}/artifacts?per_page=100")
}

pub(crate) fn artifact_route(id: u64) -> String {
    format!("repos/{REPO}/actions/artifacts/{}", 5000 + id)
}

pub(crate) fn zip_route(id: u64) -> String {
    format!("{}/zip", artifact_route(id))
}

pub(crate) fn repo_route() -> String {
    format!("repos/{REPO}")
}

pub(crate) fn branch_route() -> String {
    format!("repos/{REPO}/branches/main")
}
