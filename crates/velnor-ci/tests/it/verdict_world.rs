// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;
use std::fs;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::process::{Command, Output};
use std::thread;

use serde_json::{Value, json};

use crate::io::Repo;
use crate::resolve_wire::zip_of;

pub(crate) const REPOSITORY: &str = "tailrocks/velnor-new";
pub(crate) const HEAD: &str = "1111111111111111111111111111111111111111";
pub(crate) const BASE: &str = "2222222222222222222222222222222222222222";
pub(crate) const PLAN: &str = r#"{"rust_root_mode":"some","rust_root_tests":"run"}"#;
const RUN: u64 = 100;
const ARTIFACT: u64 = 5000;

pub(crate) struct Reuse {
    pub(crate) output: Output,
    pub(crate) outputs: BTreeMap<String, String>,
}

impl Reuse {
    pub(crate) fn reused(&self) -> Vec<&str> {
        self.outputs["reused"].split_whitespace().collect()
    }

    pub(crate) fn stdout(&self) -> String {
        String::from_utf8_lossy(&self.output.stdout).into_owned()
    }
}

pub(crate) struct World {
    pub(crate) checkout: Repo,
    pub(crate) plan: String,
    pub(crate) event: String,
    pub(crate) routes: BTreeMap<String, Vec<u8>>,
    pub(crate) overrides: Vec<(String, String)>,
}

pub(crate) fn path(tail: &str) -> String {
    format!("/repos/{REPOSITORY}{tail}")
}

pub(crate) fn json_body(value: &Value) -> Vec<u8> {
    value.to_string().into_bytes()
}

pub(crate) fn checkout(content: &str) -> Repo {
    let repo = Repo::new("verdict-checkout");
    repo.write("src.txt", content);
    repo.commit();
    repo
}

pub(crate) fn tree(repo: &Repo) -> String {
    let output = Command::new("git")
        .arg("-C")
        .arg(repo.root())
        .args(["rev-parse", "HEAD^{tree}"])
        .output()
        .unwrap_or_else(|e| unreachable!("{e}"));
    String::from_utf8_lossy(&output.stdout).trim().to_owned()
}

pub(crate) fn recorded(repo: &Repo, plan: &str) -> String {
    let temp = repo.root().join("runner-temp");
    let output = Command::new(env!("CARGO_BIN_EXE_velnor-ci"))
        .current_dir(repo.root())
        .args(["verdict", "record"])
        .env("PLAN_JSON", plan)
        .env("RUNNER_TEMP", &temp)
        .env("GITHUB_RUN_ATTEMPT", "1")
        .output()
        .unwrap_or_else(|e| unreachable!("{e}"));
    assert!(output.status.success(), "{output:?}");
    fs::read_to_string(temp.join("velnor-verdict/verdict.json"))
        .unwrap_or_else(|e| unreachable!("{e}"))
}

type Job<'a> = (&'a str, &'a str, &'a [(&'a str, &'a str)]);

pub(crate) fn jobs_on(entries: &[Job<'_>], attempt: u32) -> Vec<u8> {
    let jobs: Vec<Value> = entries
        .iter()
        .map(|(name, conclusion, steps)| {
            let steps: Vec<Value> = steps
                .iter()
                .map(|(step, outcome)| json!({"name": step, "conclusion": outcome}))
                .collect();
            json!({"id": 9, "name": name, "conclusion": conclusion, "run_attempt": attempt,
                "created_at": "2026-11-20T00:00:00Z", "steps": steps})
        })
        .collect();
    json_body(&json!({"total_count": jobs.len(), "jobs": jobs}))
}

pub(crate) fn jobs(entries: &[Job<'_>]) -> Vec<u8> {
    jobs_on(entries, 1)
}

impl World {
    pub(crate) fn new() -> Self {
        let checkout = checkout("same content");
        let mut world = Self {
            checkout,
            plan: PLAN.to_owned(),
            event: "merge_group".to_owned(),
            routes: BTreeMap::new(),
            overrides: Vec::new(),
        };
        world.serve_green_pull_request();
        world
    }

    pub(crate) fn serve_green_pull_request(&mut self) {
        let tree = tree(&self.checkout);
        let routes = [
            (
                path("/pulls/7"),
                json_body(&json!({"head": {"sha": HEAD, "repo": {"full_name": REPOSITORY}}})),
            ),
            (
                path(&format!("/compare/{BASE}...{HEAD}?per_page=1")),
                json_body(&json!({"behind_by": 0})),
            ),
            (
                path(&format!("/git/commits/{HEAD}")),
                json_body(&json!({"tree": {"sha": tree}})),
            ),
            (
                path("/rules/branches/main?per_page=100&page=1"),
                json_body(&json!([{"type": "non_fast_forward"}])),
            ),
            (
                path(&format!(
                    "/actions/workflows/ci.yml/runs?head_sha={HEAD}&event=pull_request&per_page=100"
                )),
                json_body(&json!({"total_count": 1, "workflow_runs": [{
                    "id": RUN, "event": "pull_request", "status": "completed",
                    "head_sha": HEAD, "pull_requests": [{"number": 7}]}]})),
            ),
            (
                path(&format!(
                    "/actions/runs/{RUN}/jobs?filter=latest&per_page=100&page=1"
                )),
                jobs(&[
                    ("Required", "success", &[]),
                    (
                        "rust",
                        "success",
                        &[("Format", "success"), ("Test", "success")],
                    ),
                    ("Alint", "success", &[]),
                ]),
            ),
            (
                path(&format!(
                    "/actions/runs/{RUN}/artifacts?per_page=100&page=1"
                )),
                json_body(&json!({"total_count": 1, "artifacts": [
                    {"id": ARTIFACT, "name": "velnor-verdict", "expired": false}]})),
            ),
            (
                path(&format!("/actions/artifacts/{ARTIFACT}/zip")),
                zip_of("verdict.json", recorded(&self.checkout, PLAN).as_bytes()),
            ),
        ];
        self.routes = routes.into_iter().collect();
    }

    pub(crate) fn run(&self) -> Reuse {
        let routes = self.routes.clone();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap_or_else(|e| unreachable!("{e}"));
        let address = listener
            .local_addr()
            .unwrap_or_else(|e| unreachable!("{e}"));
        thread::spawn(move || {
            for mut stream in listener.incoming().flatten() {
                let mut request = Vec::new();
                let mut byte = [0_u8; 1];
                while !request.ends_with(b"\r\n\r\n")
                    && stream.read(&mut byte).is_ok_and(|n| n == 1)
                {
                    request.push(byte[0]);
                }
                let text = String::from_utf8_lossy(&request).into_owned();
                let target = text.split_whitespace().nth(1).unwrap_or_default();
                let (status, body) = routes
                    .get(target)
                    .map_or(("404 Not Found", Vec::new()), |body| {
                        ("200 OK", body.clone())
                    });
                let head = format!(
                    "HTTP/1.1 {status}\r\ncontent-length: {}\r\nconnection: close\r\n\r\n",
                    body.len()
                );
                stream.write_all(head.as_bytes()).ok();
                stream.write_all(&body).ok();
            }
        });
        let temp = self.checkout.root().join("reuse-temp");
        fs::create_dir_all(&temp).unwrap_or_else(|e| unreachable!("{e}"));
        let outputs = temp.join("github-output");
        let reusable = json!([
            {"id": "rust.format", "job": "rust", "step": "Format"},
            {"id": "rust.test", "job": "rust", "step": "Test"},
            {"id": "lint-alint", "job": "Alint"},
        ]);
        let output = Command::new(env!("CARGO_BIN_EXE_velnor-ci"))
            .current_dir(self.checkout.root())
            .args(["verdict", "reuse"])
            .env("GITHUB_EVENT_NAME", &self.event)
            .env("GITHUB_REPOSITORY", REPOSITORY)
            .env(
                "GITHUB_WORKFLOW_REF",
                format!("{REPOSITORY}/.github/workflows/ci.yml@refs/heads/main"),
            )
            .env(
                "VELNOR_MERGE_GROUP_HEAD_REF",
                format!("gh-readonly-queue/main/pr-7-{HEAD}"),
            )
            .env("VELNOR_MERGE_GROUP_BASE_SHA", BASE)
            .env("VELNOR_REQUIRED_CHECK", "Required")
            .env("VELNOR_REUSABLE", reusable.to_string())
            .env("GITHUB_TOKEN", "token")
            .env("GITHUB_API_URL", format!("http://{address}"))
            .env("GITHUB_SERVER_URL", "https://github.com")
            .env("GITHUB_OUTPUT", &outputs)
            .env("PLAN_JSON", &self.plan)
            .env("RUNNER_TEMP", &temp)
            .env("GITHUB_RUN_ATTEMPT", "1")
            .envs(self.overrides.clone())
            .output()
            .unwrap_or_else(|e| unreachable!("{e}"));
        let written = fs::read_to_string(&outputs).unwrap_or_default();
        let outputs = written
            .lines()
            .filter_map(|line| line.split_once('='))
            .map(|(key, value)| (key.to_owned(), value.to_owned()))
            .collect();
        Reuse { output, outputs }
    }
}
