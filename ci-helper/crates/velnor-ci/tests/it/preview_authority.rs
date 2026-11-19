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
use velnor_ci::preview::json::{Res, canonical, digest};
use velnor_ci::preview::net::Net;

use crate::preview_world::{R, REPO, SIGNER, SOURCE, TAG_OID, darwin_payload, linux_payload, sha};
use crate::support::fresh_dir;

const MAIN: &str = "5555555555555555555555555555555555555555";
const SIGNING: &str = "6666666666666666666666666666666666666666";
const PUBLICATION: &str = ".github/workflows/source-preview-publication.yml";
const LINUX_NAME: &str = "velnor-actions-x86_64-unknown-linux-gnu";
const DARWIN_NAME: &str = "velnor-actions-aarch64-apple-darwin";

#[derive(Debug)]
struct FakeClient {
    api: RefCell<BTreeMap<String, Value>>,
    zips: BTreeMap<u64, Vec<u8>>,
    writes: RefCell<Vec<(String, String)>>,
    deadline: Instant,
}

impl FakeClient {
    fn set(&self, suffix: &str, value: Value) {
        self.api.borrow_mut().insert(format!("{R}{suffix}"), value);
    }

    fn update(&self, suffix: &str, change: impl FnOnce(&mut Value)) {
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
struct Accepting {
    subjects: RefCell<Vec<Subject>>,
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

fn metadata() -> Value {
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

struct Scenario {
    base: PathBuf,
    temp: PathBuf,
    client: FakeClient,
    vars: BTreeMap<String, String>,
}

fn job(id: u64, attempt: u64, name: &str, conclusion: &str) -> Value {
    json!({"id": id, "run_id": 500, "run_attempt": attempt, "head_sha": SOURCE, "name": name, "status": "completed", "conclusion": conclusion})
}

fn write(path: &Path, data: &[u8]) {
    fs::create_dir_all(path.parent().unwrap_or(path)).unwrap_or_else(|e| unreachable!("{e}"));
    fs::write(path, data).unwrap_or_else(|e| unreachable!("{e}"));
}

impl Scenario {
    fn new(name: &str, attempt: u64) -> Self {
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

    fn run(&self, mode: Mode) -> Result<(), String> {
        let attestor = Accepting {
            subjects: RefCell::new(Vec::new()),
        };
        let inputs = Inputs {
            vars: self.vars.clone(),
        };
        run(mode, &self.client, &attestor, &inputs, &self.base).map_err(|e| e.to_string())
    }

    fn outputs(&self) -> String {
        fs::read_to_string(&self.vars["GITHUB_OUTPUT"]).unwrap_or_default()
    }

    fn candidate() -> Value {
        json!({"source_artifact_id": 101, "linux_payload_artifact_id": 102, "darwin_payload_artifact_id": 103})
    }

    fn draft_release(&self) {
        let marker = candidate_marker(&metadata(), &Self::candidate())
            .unwrap_or_else(|e| unreachable!("{e}"));
        self.client.set("/releases/9000", json!({"id": 9000, "tag_name": format!("sdk-preview-{SOURCE}"), "body": marker, "draft": true, "prerelease": true, "assets": []}));
    }
}

#[test]
fn draft_creates_the_draft_release_and_asks_to_sign() {
    let s = Scenario::new("draft", 2);
    s.client.set(
        "/actions/runs/500/artifacts?per_page=100&page=1",
        json!({"total_count": 0, "artifacts": []}),
    );
    s.run(Mode::Draft).unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(
        s.outputs(),
        "darwin_payload_artifact_id=103\nlinux_payload_artifact_id=102\nsource_artifact_id=101\nsign=true\n"
    );
    assert_eq!(
        s.client.writes.borrow().as_slice(),
        [("POST".to_owned(), "/releases".to_owned())]
    );
    let decision: Value = serde_json::from_slice(
        &fs::read(s.temp.join("source-preview-decision.json")).unwrap_or_default(),
    )
    .unwrap_or_default();
    assert_eq!(decision["release_id"], 9000);
    assert_eq!(decision["draft"]["run_attempt"], 2);
    assert_eq!(
        decision["targets"]["linux"]["payload"]["sha256"],
        digest(&linux_payload())
    );
}

#[test]
fn a_second_draft_in_the_same_run_does_not_overwrite_the_decision() {
    let s = Scenario::new("draft-twice", 2);
    s.client.set(
        "/actions/runs/500/artifacts?per_page=100&page=1",
        json!({"total_count": 0, "artifacts": []}),
    );
    s.run(Mode::Draft).unwrap_or_else(|e| unreachable!("{e}"));
    s.client.set(
        "/releases?per_page=100&page=1",
        json!([{"id": 9000, "tag_name": format!("sdk-preview-{SOURCE}")}]),
    );
    assert!(s.run(Mode::Draft).is_err());
}

fn signed_up(attempt: u64) -> Scenario {
    let s = Scenario::new(&format!("flow{attempt}"), 2);
    s.client.set(
        "/actions/runs/500/artifacts?per_page=100&page=1",
        json!({"total_count": 0, "artifacts": []}),
    );
    s.run(Mode::Draft).unwrap_or_else(|e| unreachable!("{e}"));
    let work = &s.base;
    fs::copy(s.temp.join("source-preview-decision.json"), {
        let target = work.join("source-preview-decision/source-preview-decision.json");
        write(&target, b"");
        target
    })
    .unwrap_or_else(|e| unreachable!("{e}"));
    let mut next = Scenario::new(&format!("flow{attempt}-prepare"), attempt);
    next.base = s.base.clone();
    next.client = FakeClient {
        api: RefCell::new(s.client.api.borrow().clone()),
        zips: BTreeMap::new(),
        writes: RefCell::new(Vec::new()),
        deadline: s.client.deadline,
    };
    next
}

#[test]
fn prepare_writes_the_receipt_the_attest_job_signs() {
    let mut s = signed_up(3);
    s.vars.insert("SIGN".to_owned(), "true".to_owned());
    s.run(Mode::Prepare).unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(s.outputs(), "sign=true\n");
    let bytes =
        fs::read(s.temp.join("SDK_PREVIEW_RECEIPT.json")).unwrap_or_else(|e| unreachable!("{e}"));
    let receipt: Value = serde_json::from_slice(&bytes).unwrap_or_default();
    assert_eq!(canonical(&receipt), bytes);
    assert_eq!(receipt["signer"]["run_attempt"], 3);
    assert_eq!(receipt["draft"]["run_attempt"], 2);
    assert_eq!(receipt["source"]["run_attempt"], 1);
    assert_eq!(receipt["release_id"], 9000);
    assert_eq!(receipt["candidate_source_artifact_id"], 101);
    assert_eq!(
        receipt["targets"]["darwin"]["candidate_payload_artifact_id"],
        103
    );
    assert!(
        !s.temp
            .join("SDK_PREVIEW_RECEIPT.json.sigstore.jsonl")
            .exists()
    );
}

#[test]
fn publish_uploads_the_four_assets_and_publishes_an_immutable_release() {
    let mut s = signed_up(3);
    s.vars.insert("SIGN".to_owned(), "true".to_owned());
    s.run(Mode::Prepare).unwrap_or_else(|e| unreachable!("{e}"));
    let receipt = fs::read(s.temp.join("SDK_PREVIEW_RECEIPT.json")).unwrap_or_default();
    write(
        &s.base
            .join("source-preview-signed/SDK_PREVIEW_RECEIPT.json"),
        &receipt,
    );
    write(
        &s.base
            .join("source-preview-signed/SDK_PREVIEW_RECEIPT.json.sigstore.jsonl"),
        b"{\"bundle\":1}\n",
    );
    s.vars
        .insert("PAIR_ORIGIN".to_owned(), "artifact".to_owned());
    s.vars.insert("PAIR_ID".to_owned(), "77".to_owned());
    s.run(Mode::Publish).unwrap_or_else(|e| unreachable!("{e}"));
    let writes = s.client.writes.borrow();
    let names: Vec<&str> = writes
        .iter()
        .filter(|(k, _)| k == "UPLOAD")
        .map(|(_, n)| n.as_str())
        .collect();
    assert_eq!(
        names,
        [
            "SDK_PREVIEW_RECEIPT.json",
            "SDK_PREVIEW_RECEIPT.json.sigstore.jsonl",
            DARWIN_NAME,
            LINUX_NAME
        ]
    );
    assert_eq!(
        writes.last(),
        Some(&("PATCH".to_owned(), "/releases/9000".to_owned()))
    );
    let release = s.client.api.borrow()[&format!("{R}/releases/9000")].clone();
    assert_eq!(release["draft"], false);
    assert_eq!(release["immutable"], true);
}

#[test]
fn publish_refuses_a_receipt_that_was_not_made_for_these_payloads() {
    let mut s = signed_up(3);
    s.vars.insert("SIGN".to_owned(), "true".to_owned());
    s.run(Mode::Prepare).unwrap_or_else(|e| unreachable!("{e}"));
    let receipt = String::from_utf8_lossy(
        &fs::read(s.temp.join("SDK_PREVIEW_RECEIPT.json")).unwrap_or_default(),
    )
    .replace(&digest(&linux_payload()), &sha(b"other"));
    write(
        &s.base
            .join("source-preview-signed/SDK_PREVIEW_RECEIPT.json"),
        receipt.as_bytes(),
    );
    write(
        &s.base
            .join("source-preview-signed/SDK_PREVIEW_RECEIPT.json.sigstore.jsonl"),
        b"x\n",
    );
    s.vars
        .insert("PAIR_ORIGIN".to_owned(), "artifact".to_owned());
    s.vars.insert("PAIR_ID".to_owned(), "77".to_owned());
    assert!(s.run(Mode::Publish).is_err());
    assert!(s.client.writes.borrow().is_empty());
}

#[test]
fn every_broken_precondition_of_draft_is_refused() {
    type Break = fn(&Scenario);
    let cases: [(&str, Break); 8] = [
        ("tag is lightweight", |s| {
            s.client
                .update(&format!("/git/ref/tags/sdk-preview-{SOURCE}"), |v| {
                    v["object"]["type"] = json!("commit")
                })
        }),
        ("main is not ahead of the source", |s| {
            s.client
                .update(&format!("/compare/{SOURCE}...{MAIN}"), |v| {
                    v["status"] = json!("diverged")
                })
        }),
        ("admit job failed", |s| {
            s.client.update(
                "/actions/runs/500/attempts/1/jobs?per_page=100&page=1",
                |v| v["jobs"][0]["conclusion"] = json!("failure"),
            )
        }),
        ("admit run is another branch", |s| {
            s.client.update("/actions/runs/500/attempts/1", |v| {
                v["head_branch"] = json!("main")
            })
        }),
        ("a release of the tag exists with another marker", |s| {
            s.client.set(
                "/releases?per_page=100&page=1",
                json!([{"id": 9000, "tag_name": format!("sdk-preview-{SOURCE}"), "body": "x"}]),
            );
            s.client
                .set("/releases/9000", json!({"id": 9000, "body": "x"}));
        }),
        ("two releases carry the tag", |s| {
            let tag = format!("sdk-preview-{SOURCE}");
            s.client.set(
                "/releases?per_page=100&page=1",
                json!([{"id": 1, "tag_name": tag}, {"id": 2, "tag_name": tag}]),
            );
        }),
        ("the release list repeats an id", |s| {
            s.client.set(
                "/releases?per_page=100&page=1",
                json!([{"id": 1, "tag_name": "a"}, {"id": 1, "tag_name": "b"}]),
            )
        }),
        ("a payload is not an executable of the target", |s| {
            write(
                &s.base
                    .join(format!("source-preview-payload-darwin/{DARWIN_NAME}")),
                &linux_payload(),
            )
        }),
    ];
    for (name, change) in cases {
        let s = Scenario::new("refuse", 2);
        s.client.set(
            "/actions/runs/500/artifacts?per_page=100&page=1",
            json!({"total_count": 0, "artifacts": []}),
        );
        change(&s);
        assert!(s.run(Mode::Draft).is_err(), "accepted: {name}");
        assert!(
            s.client.writes.borrow().is_empty(),
            "wrote before refusing: {name}"
        );
    }
}

#[test]
fn the_environment_must_be_the_tag_push_of_the_source() {
    for (key, value) in [
        ("GITHUB_EVENT_NAME", "workflow_dispatch"),
        ("GITHUB_REF", "refs/heads/main"),
        ("GITHUB_REPOSITORY", "evil/repo"),
        ("GITHUB_RUN_ID", "501"),
        ("GITHUB_RUN_ATTEMPT", "0"),
        ("GITHUB_RUN_ATTEMPT", "52"),
        ("AUTHORITY_PATH", ".github/workflows/ci.yml"),
        ("AUTHORITY_SHA", "abc"),
        ("SOURCE_ARTIFACT_ID", "0"),
        ("DARWIN_PAYLOAD_ARTIFACT_ID", "x"),
    ] {
        let mut s = Scenario::new("env", 2);
        s.client.set(
            "/actions/runs/500/artifacts?per_page=100&page=1",
            json!({"total_count": 0, "artifacts": []}),
        );
        s.vars.insert(key.to_owned(), value.to_owned());
        assert!(s.run(Mode::Draft).is_err(), "accepted {key}={value}");
    }
}

#[test]
fn the_metadata_must_be_canonical_and_the_only_file() {
    let s = Scenario::new("metadata", 2);
    let path = s
        .base
        .join("source-preview-metadata/source-preview-metadata.json");
    write(
        &path,
        serde_json::to_string_pretty(&metadata())
            .unwrap_or_default()
            .as_bytes(),
    );
    assert!(s.run(Mode::Draft).is_err());
    let s = Scenario::new("metadata-extra", 2);
    write(&s.base.join("source-preview-metadata/extra"), b"x");
    assert!(s.run(Mode::Draft).is_err());
}

#[test]
fn an_unknown_mode_is_refused() {
    assert!(Mode::parse("sign").is_err());
    assert_eq!(Mode::parse("publish").ok(), Some(Mode::Publish));
}

#[test]
fn draft_reuses_the_candidate_of_an_existing_draft_release_only_when_every_artifact_matches() {
    let s = Scenario::new("existing", 2);
    s.draft_release();
    s.client.set(
        "/releases?per_page=100&page=1",
        json!([{"id": 9000, "tag_name": format!("sdk-preview-{SOURCE}")}]),
    );
    assert!(
        s.run(Mode::Draft).is_err(),
        "the candidate artifacts are not there"
    );
}

fn python_ready() -> bool {
    std::process::Command::new("python3")
        .args(["-I", "-S", "-c", "import json"])
        .output()
        .is_ok_and(|out| out.status.success())
}

/// Run the legacy script for `mode` in the scenario's directory, with its environment.
fn legacy(s: &Scenario, mode: &str) -> Value {
    let renderer = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../../crates/velnor-actions-workflow-renderer/src");
    let harness =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/it/data/preview_parity/run_authority.py");
    let world = s.base.join("../world.json");
    fs::write(&world, json!({"api": *s.client.api.borrow()}).to_string())
        .unwrap_or_else(|e| unreachable!("{e}"));
    let mut command = std::process::Command::new("python3");
    command
        .args(["-I", "-S"])
        .arg(harness)
        .arg(renderer)
        .arg(&world)
        .current_dir(&s.base)
        .env_clear();
    for (key, value) in &s.vars {
        command.env(key, value);
    }
    let out = command
        .env("PREVIEW_MODE", mode)
        .env("GH_TOKEN", "x")
        .output()
        .unwrap_or_else(|e| unreachable!("{e}"));
    let text = String::from_utf8_lossy(&out.stdout).into_owned();
    let value: Value = serde_json::from_str(text.trim())
        .unwrap_or_else(|e| unreachable!("{e}: {text} {}", String::from_utf8_lossy(&out.stderr)));
    assert_eq!(value["ok"], true, "the legacy script refused");
    value
}

#[test]
fn preview_parity_authority_draft_and_prepare_write_what_the_legacy_script_writes() {
    if !python_ready() {
        eprintln!("skipped: python3 is not available");
        return;
    }
    let artifacts = json!({"total_count": 0, "artifacts": []});
    let old = Scenario::new("parity-old-draft", 2);
    old.client.set(
        "/actions/runs/500/artifacts?per_page=100&page=1",
        artifacts.clone(),
    );
    let created = legacy(&old, "draft")["release"].clone();
    let new = Scenario::new("parity-new-draft", 2);
    new.client
        .set("/actions/runs/500/artifacts?per_page=100&page=1", artifacts);
    new.run(Mode::Draft).unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(old.outputs(), new.outputs());
    let decision =
        |s: &Scenario| fs::read(s.temp.join("source-preview-decision.json")).unwrap_or_default();
    assert_eq!(decision(&old), decision(&new));
    assert!(!decision(&new).is_empty());
    assert_eq!(
        created,
        new.client.api.borrow()[&format!("{R}/releases/9000")]
    );
    for (label, scenario) in [("old", &old), ("new", &new)] {
        let target = scenario
            .base
            .join("source-preview-decision/source-preview-decision.json");
        write(&target, &decision(scenario));
        assert!(target.is_file(), "{label}");
    }
    for (scenario, python) in [(&old, true), (&new, false)] {
        let next = Scenario::new(
            if python {
                "parity-old-prepare"
            } else {
                "parity-new-prepare"
            },
            3,
        );
        fs::remove_dir_all(&next.base).unwrap_or_else(|e| unreachable!("{e}"));
        fs::rename(&scenario.base, &next.base).unwrap_or_else(|e| unreachable!("{e}"));
        let mut next = next;
        next.client = FakeClient {
            api: RefCell::new(new.client.api.borrow().clone()),
            zips: BTreeMap::new(),
            writes: RefCell::new(Vec::new()),
            deadline: scenario.client.deadline,
        };
        next.vars.insert("SIGN".to_owned(), "true".to_owned());
        if python {
            legacy(&next, "prepare");
        } else {
            next.run(Mode::Prepare)
                .unwrap_or_else(|e| unreachable!("{e}"));
        }
        let receipt = fs::read(next.temp.join("SDK_PREVIEW_RECEIPT.json")).unwrap_or_default();
        assert!(!receipt.is_empty());
        fs::write(scenario.temp.join("receipt.copy"), &receipt)
            .unwrap_or_else(|e| unreachable!("{e}"));
        fs::write(scenario.temp.join("prepare-outputs.copy"), next.outputs())
            .unwrap_or_else(|e| unreachable!("{e}"));
    }
    let copy = |s: &Scenario, name: &str| fs::read(s.temp.join(name)).unwrap_or_default();
    assert_eq!(copy(&old, "receipt.copy"), copy(&new, "receipt.copy"));
    assert_eq!(
        copy(&old, "prepare-outputs.copy"),
        copy(&new, "prepare-outputs.copy")
    );
}
