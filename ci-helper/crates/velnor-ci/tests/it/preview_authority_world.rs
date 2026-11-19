// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use serde_json::{Value, json};
use velnor_ci::error::CiError;
use velnor_ci::preview::attest::{Attestor, Authority, Subject};
use velnor_ci::preview::authority::Client;
use velnor_ci::preview::authority::model::candidate_marker;
use velnor_ci::preview::authority::modes::{Inputs, Mode, run};
use velnor_ci::preview::json::{Res, canonical};
use velnor_ci::preview::net::Net;

use crate::preview_world::{R, REPO, SIGNER, SOURCE, TAG_OID, darwin_payload, linux_payload, sha};
use crate::support::fresh_dir;

pub(crate) const MAIN: &str = "5555555555555555555555555555555555555555";
pub(crate) const SIGNING: &str = "6666666666666666666666666666666666666666";
pub(crate) const PUBLICATION: &str = ".github/workflows/source-preview-publication.yml";
pub(crate) const LINUX_NAME: &str = "velnor-actions-x86_64-unknown-linux-gnu";
pub(crate) const DARWIN_NAME: &str = "velnor-actions-aarch64-apple-darwin";

#[derive(Debug)]
pub(crate) struct FakeClient {
    pub(crate) api: RefCell<BTreeMap<String, Value>>,
    pub(crate) zips: BTreeMap<u64, Vec<u8>>,
    pub(crate) writes: RefCell<Vec<(String, String)>>,
    pub(crate) deadline: Instant,
}

impl FakeClient {
    pub(crate) fn set(&self, suffix: &str, value: Value) {
        self.api.borrow_mut().insert(format!("{R}{suffix}"), value);
    }

    pub(crate) fn update(&self, suffix: &str, change: impl FnOnce(&mut Value)) {
        let mut api = self.api.borrow_mut();
        change(
            api.get_mut(&format!("{R}{suffix}"))
                .unwrap_or_else(|| unreachable!("{suffix}")),
        );
    }
}

impl Net for FakeClient {
    fn api(&self, path: &str) -> Res<Value> {
        self.api
            .borrow()
            .get(path)
            .cloned()
            .ok_or_else(|| CiError::new(format!("fake: no {path}")))
    }

    fn asset(&self, url: &str, _maximum: usize) -> Res<Vec<u8>> {
        Err(CiError::new(format!("fake: no asset {url}")))
    }

    fn deadline(&self) -> Instant {
        self.deadline
    }
}

impl Client for FakeClient {
    fn send(&self, method: &str, suffix: &str, body: &[u8], allowed: &[u16]) -> Res<Value> {
        self.writes
            .borrow_mut()
            .push((method.to_owned(), suffix.to_owned()));
        let parsed: Value = serde_json::from_slice(body).unwrap_or(Value::Null);
        assert_eq!(canonical(&parsed), body, "request bodies are canonical");
        match (method, suffix) {
            ("POST", "/releases") => {
                assert_eq!(allowed, [201]);
                assert_eq!(parsed["draft"], true);
                assert_eq!(parsed["prerelease"], true);
                assert_eq!(parsed["target_commitish"], SOURCE);
                let item = json!({"id": 9000, "tag_name": parsed["tag_name"], "body": parsed["body"], "draft": true, "prerelease": true, "assets": []});
                self.set("/releases/9000", item.clone());
                Ok(item)
            }
            ("PATCH", "/releases/9000") => {
                assert_eq!(parsed, json!({"draft": false}));
                self.update("/releases/9000", |v| {
                    v["draft"] = json!(false);
                    v["immutable"] = json!(true);
                });
                Ok(json!({}))
            }
            _ => Err(CiError::new("fake: unexpected write")),
        }
    }

    fn upload(&self, release_id: u64, name: &str, data: &[u8]) -> Res<Value> {
        self.writes
            .borrow_mut()
            .push(("UPLOAD".to_owned(), name.to_owned()));
        assert_eq!(release_id, 9000);
        let item = json!({"name": name, "state": "uploaded", "size": data.len(), "digest": format!("sha256:{}", sha(data))});
        self.update("/releases/9000", |v| {
            v["assets"]
                .as_array_mut()
                .unwrap_or_else(|| unreachable!())
                .push(item.clone());
        });
        Ok(item)
    }

    fn artifact_zip(&self, id: u64, _size: u64, _digest: &str) -> Res<Vec<u8>> {
        self.zips
            .get(&id)
            .cloned()
            .ok_or_else(|| CiError::new("fake: no artifact"))
    }
}

#[derive(Debug)]
pub(crate) struct Accepting {
    pub(crate) subjects: RefCell<Vec<Subject>>,
}

impl Attestor for Accepting {
    fn verify(
        &self,
        _r: &[u8],
        _b: &[u8],
        subject: &Subject,
        authority: &Authority,
        _d: Instant,
    ) -> Res<Value> {
        assert_eq!(authority.sha, SIGNING);
        self.subjects.borrow_mut().push(subject.clone());
        Ok(json!([]))
    }
}

pub(crate) fn metadata() -> Value {
    let tag = format!("sdk-preview-{SOURCE}");
    let run = |path: &str, git_ref: &str, id: u64| json!({"workflow_path": path, "workflow_ref": format!("{REPO}/{path}@{git_ref}"), "workflow_sha": SOURCE, "run_id": id, "run_attempt": 1});
    let target = |run_id: u64, artifact: u64, job: u64, label: &str| {
        json!({"producer": {"producer_run_id": run_id, "producer_run_attempt": 1, "payload_artifact_id": artifact,
                            "payload_archive_sha256": sha(label.as_bytes())}, "qualification_job_id": job})
    };
    json!({"schema": 3, "repository": REPO, "version": "0.1.82", "source_sha": SOURCE, "tag": tag,
           "source": run(SIGNER, &format!("refs/tags/{tag}"), 500),
           "ci": run(".github/workflows/ci.yml", "refs/heads/main", 600),
           "qualification": run(".github/workflows/source-qualification.yml", "refs/heads/main", 700),
           "targets": {"linux": target(800, 11, 8101, "linux"), "darwin": target(900, 13, 9101, "darwin")}})
}

pub(crate) struct Scenario {
    pub(crate) base: PathBuf,
    pub(crate) temp: PathBuf,
    pub(crate) client: FakeClient,
    pub(crate) vars: BTreeMap<String, String>,
}

pub(crate) fn job(id: u64, attempt: u64, name: &str, conclusion: &str) -> Value {
    json!({"id": id, "run_id": 500, "run_attempt": attempt, "head_sha": SOURCE, "name": name, "status": "completed", "conclusion": conclusion})
}

pub(crate) fn write(path: &Path, data: &[u8]) {
    fs::create_dir_all(path.parent().unwrap_or(path)).unwrap_or_else(|e| unreachable!("{e}"));
    fs::write(path, data).unwrap_or_else(|e| unreachable!("{e}"));
}

pub(crate) fn fake_client(tag: &str) -> FakeClient {
    let client = FakeClient {
        api: RefCell::new(BTreeMap::new()),
        zips: BTreeMap::new(),
        writes: RefCell::new(Vec::new()),
        deadline: Instant::now() + Duration::from_secs(180),
    };
    client.set(
        &format!("/git/ref/tags/{tag}"),
        json!({"object": {"type": "tag", "sha": TAG_OID}}),
    );
    client.set(
        &format!("/git/tags/{TAG_OID}"),
        json!({"tag": tag, "object": {"type": "commit", "sha": SOURCE}}),
    );
    client.set(
        "/git/ref/heads/main",
        json!({"object": {"type": "commit", "sha": MAIN}}),
    );
    client.set(
        &format!("/compare/{SOURCE}...{MAIN}"),
        json!({"status": "ahead", "merge_base_commit": {"sha": SOURCE}}),
    );
    for (run_attempt, name) in [(1, "Admit"), (2, "Draft"), (3, "Attest")] {
        client.set(
            &format!("/actions/runs/500/attempts/{run_attempt}"),
            json!({"id": 500, "run_attempt": run_attempt, "head_sha": SOURCE, "head_branch": tag, "event": "push",
                   "path": format!("{SIGNER}@refs/tags/{tag}"), "head_repository": {"full_name": REPO}}),
        );
        let full = format!("Source preview publication / {name} source preview");
        client.set(&format!("/actions/runs/500/attempts/{run_attempt}/jobs?per_page=100&page=1"), json!({"total_count": 1, "jobs": [job(5000 + run_attempt, run_attempt, &full, "success")]}));
    }
    client.set("/releases?per_page=100&page=1", json!([]));
    client
}

impl Scenario {
    pub(crate) fn new(name: &str, attempt: u64) -> Self {
        let root = fs::canonicalize(fresh_dir("velnor-ci-authority", name))
            .unwrap_or_else(|e| unreachable!("{e}"));
        let (base, temp) = (root.join("work"), root.join("temp"));
        fs::create_dir_all(&temp).unwrap_or_else(|e| unreachable!("{e}"));
        let output = root.join("output");
        write(&output, b"");
        write(
            &base.join("source-preview-metadata/source-preview-metadata.json"),
            &canonical(&metadata()),
        );
        write(
            &base.join(format!("source-preview-payload-linux/{LINUX_NAME}")),
            &linux_payload(),
        );
        write(
            &base.join(format!("source-preview-payload-darwin/{DARWIN_NAME}")),
            &darwin_payload(),
        );
        let tag = format!("sdk-preview-{SOURCE}");
        let client = fake_client(&tag);
        let vars: BTreeMap<String, String> = [
            ("GITHUB_SHA", SOURCE.to_owned()),
            ("GITHUB_REPOSITORY", REPO.to_owned()),
            ("GITHUB_EVENT_NAME", "push".to_owned()),
            ("GITHUB_REF", format!("refs/tags/{tag}")),
            (
                "GITHUB_WORKFLOW_REF",
                format!("{REPO}/{SIGNER}@refs/tags/{tag}"),
            ),
            ("GITHUB_WORKFLOW_SHA", SOURCE.to_owned()),
            ("GITHUB_RUN_ID", "500".to_owned()),
            ("GITHUB_RUN_ATTEMPT", attempt.to_string()),
            ("RUNNER_TEMP", temp.display().to_string()),
            ("GITHUB_OUTPUT", output.display().to_string()),
            ("SOURCE_ARTIFACT_ID", "101".to_owned()),
            ("LINUX_PAYLOAD_ARTIFACT_ID", "102".to_owned()),
            ("DARWIN_PAYLOAD_ARTIFACT_ID", "103".to_owned()),
            ("AUTHORITY_REPOSITORY", REPO.to_owned()),
            ("AUTHORITY_PATH", PUBLICATION.to_owned()),
            ("AUTHORITY_REF", format!("{REPO}/{PUBLICATION}@{SIGNING}")),
            ("AUTHORITY_SHA", SIGNING.to_owned()),
        ]
        .into_iter()
        .map(|(k, v)| (k.to_owned(), v))
        .collect();
        Self {
            base,
            temp,
            client,
            vars,
        }
    }

    pub(crate) fn run(&self, mode: Mode) -> Result<(), String> {
        let attestor = Accepting {
            subjects: RefCell::new(Vec::new()),
        };
        let inputs = Inputs {
            vars: self.vars.clone(),
        };
        run(mode, &self.client, &attestor, &inputs, &self.base).map_err(|e| e.to_string())
    }

    pub(crate) fn outputs(&self) -> String {
        fs::read_to_string(&self.vars["GITHUB_OUTPUT"]).unwrap_or_default()
    }

    pub(crate) fn candidate() -> Value {
        json!({"source_artifact_id": 101, "linux_payload_artifact_id": 102, "darwin_payload_artifact_id": 103})
    }

    pub(crate) fn draft_release(&self) {
        let marker = candidate_marker(&metadata(), &Self::candidate())
            .unwrap_or_else(|e| unreachable!("{e}"));
        self.client.set("/releases/9000", json!({"id": 9000, "tag_name": format!("sdk-preview-{SOURCE}"), "body": marker, "draft": true, "prerelease": true, "assets": []}));
    }
}
