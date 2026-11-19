// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

//! A complete, valid GitHub as the source preview checks see it, for the tests of
//! `verify-runtime` and the parity test against the legacy Python.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::time::{Duration, Instant};

use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};
use velnor_ci::error::CiError;
use velnor_ci::preview::attest::{Attestor, Authority, Subject};
use velnor_ci::preview::json::Res;
use velnor_ci::preview::net::Net;

pub(crate) const REPO: &str = "tailrocks/velnor-new";
pub(crate) const R: &str = "/repos/tailrocks/velnor-new";
pub(crate) const SOURCE: &str = "1111111111111111111111111111111111111111";
pub(crate) const PUBLIC: &str = "2222222222222222222222222222222222222222";
pub(crate) const PARENT: &str = "3333333333333333333333333333333333333333";
pub(crate) const TAG_OID: &str = "4444444444444444444444444444444444444444";
pub(crate) const AUTHORITY: &str = "72701af69f6f9b693e639814fcf479409da3295c";
pub(crate) const SIGNER: &str = ".github/workflows/product-release-generator.yml";
pub(crate) const PUBLIC_RUN: u64 = 1000;

pub(crate) fn sha(data: &[u8]) -> String {
    Sha256::digest(data)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

pub(crate) fn linux_payload() -> Vec<u8> {
    let mut data = b"\x7fELF\x02\x01\x01".to_vec();
    data.resize(64, 0);
    data[18] = 0x3e;
    data
}

pub(crate) fn darwin_payload() -> Vec<u8> {
    let mut data = b"\xcf\xfa\xed\xfe\x0c\x00\x00\x01".to_vec();
    data.resize(64, 0);
    data[12] = 2;
    data
}

fn workflow_ref(path: &str, git_ref: &str) -> String {
    format!("{REPO}/{path}@{git_ref}")
}

fn record(receipt: &[u8], bundle: &[u8]) -> Value {
    let tag = format!("sdk-preview-{SOURCE}");
    let tag_ref = format!("refs/tags/{tag}");
    let mut map = Map::new();
    let mut put = |key: &str, value: Value| {
        map.insert(key.to_owned(), value);
    };
    put("schema", json!(3));
    put("repository", json!(REPO));
    put("version", json!("0.1.82"));
    put("source_sha", json!(SOURCE));
    put("tag", json!(tag));
    put("release_id", json!(9000));
    put("receipt_sha256", json!(sha(receipt)));
    put("bundle_sha256", json!(sha(bundle)));
    put("candidate_source_artifact_id", json!(101));
    put("publisher_run_attempt", json!(3));
    put("publisher_job_id", json!(7003));
    for (role, path, git_ref, run, attempt) in [
        ("source", SIGNER, tag_ref.as_str(), 500, 1),
        ("draft", SIGNER, tag_ref.as_str(), 500, 2),
        ("signer", SIGNER, tag_ref.as_str(), 500, 3),
        ("ci", ".github/workflows/ci.yml", "refs/heads/main", 600, 1),
        (
            "qualification",
            ".github/workflows/source-qualification.yml",
            "refs/heads/main",
            700,
            1,
        ),
    ] {
        put(&format!("{role}_workflow_path"), json!(path));
        put(
            &format!("{role}_workflow_ref"),
            json!(workflow_ref(path, git_ref)),
        );
        put(&format!("{role}_workflow_sha"), json!(SOURCE));
        put(&format!("{role}_run_id"), json!(run));
        put(&format!("{role}_run_attempt"), json!(attempt));
    }
    for (key, target, name, data, run, artifact, candidate, job) in [
        (
            "linux",
            "x86_64-unknown-linux-gnu",
            "velnor-actions-x86_64-unknown-linux-gnu",
            linux_payload(),
            800,
            11,
            12,
            8101,
        ),
        (
            "darwin",
            "aarch64-apple-darwin",
            "velnor-actions-aarch64-apple-darwin",
            darwin_payload(),
            900,
            13,
            14,
            9101,
        ),
    ] {
        put(&format!("{key}_producer_run_id"), json!(run));
        put(&format!("{key}_producer_run_attempt"), json!(1));
        put(
            &format!("{key}_producer_payload_artifact_id"),
            json!(artifact),
        );
        put(
            &format!("{key}_producer_payload_archive_sha256"),
            json!(sha(key.as_bytes())),
        );
        put(
            &format!("{key}_candidate_payload_artifact_id"),
            json!(candidate),
        );
        put(&format!("{key}_qualification_job_id"), json!(job));
        put(&format!("{key}_payload_target"), json!(target));
        put(&format!("{key}_payload_name"), json!(name));
        put(&format!("{key}_payload_size"), json!(data.len()));
        put(&format!("{key}_payload_sha256"), json!(sha(&data)));
    }
    Value::Object(map)
}

fn toml_of(record: &Value, table: &str) -> String {
    let mut text = format!("[{table}]\n");
    for (key, value) in record.as_object().into_iter().flatten() {
        text.push_str(&format!("{key} = {value}\n"));
    }
    text
}

fn base64(data: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut text = String::new();
    for chunk in data.chunks(3) {
        let n = chunk
            .iter()
            .enumerate()
            .fold(0_u32, |n, (i, b)| n | (u32::from(*b) << (16 - 8 * i)));
        for i in 0..4 {
            if i <= chunk.len() {
                text.push(ALPHABET[((n >> (18 - 6 * i)) & 63) as usize] as char);
            } else {
                text.push('=');
            }
        }
    }
    text
}

pub(crate) fn lock_blob(text: &str) -> Value {
    json!({"type": "file", "encoding": "base64", "path": ".velnor/generator.lock", "size": text.len(), "content": base64(text.as_bytes())})
}

fn run_json(id: u64, attempt: u64, event: &str, branch: &str, path: &str) -> Value {
    json!({"id": id, "run_attempt": attempt, "event": event, "head_sha": SOURCE, "head_branch": branch,
           "path": path, "status": "completed", "conclusion": "success",
           "head_repository": {"id": 1_390_620_900_u64, "full_name": REPO}})
}

fn job(id: u64, run: u64, attempt: u64, name: &str) -> Value {
    json!({"id": id, "run_id": run, "run_attempt": attempt, "head_sha": SOURCE, "name": name,
           "status": "completed", "conclusion": "success"})
}

/// Everything GitHub answers, and the bytes behind each release asset URL.
#[derive(Debug, Clone)]
pub(crate) struct World {
    pub api: BTreeMap<String, Value>,
    pub assets: BTreeMap<String, Vec<u8>>,
    pub record: Value,
    pub attestation: Value,
}

impl World {
    pub(crate) fn path(suffix: &str) -> String {
        format!("{R}{suffix}")
    }

    pub(crate) fn new() -> Self {
        let mut world = Self {
            api: BTreeMap::new(),
            assets: BTreeMap::new(),
            record: Value::Null,
            attestation: Value::Null,
        };
        // The receipt does not hold its own digest, so it is built from a record without them.
        let first = record(b"", b"");
        let receipt = velnor_ci::preview::json::canonical(
            &velnor_ci::preview::record::receipt_from_record(&first),
        );
        let bundle = b"{\"bundle\":true}\n".to_vec();
        world.record = record(&receipt, &bundle);
        world.public();
        world.locks();
        world.runs();
        world.release(&receipt, &bundle);
        world.attestation = attestation(&world.record);
        world
    }

    fn set(&mut self, suffix: &str, value: Value) {
        self.api.insert(Self::path(suffix), value);
    }

    fn public(&mut self) {
        self.set(
            "",
            json!({"id": 1_390_620_900_u64, "full_name": REPO, "default_branch": "main"}),
        );
        self.set("/commits/main", json!({"sha": PUBLIC}));
        self.set(
            &format!("/commits/{PUBLIC}"),
            json!({"sha": PUBLIC, "parents": [{"sha": PARENT}]}),
        );
        self.set(
            &format!("/actions/runs/{PUBLIC_RUN}/attempts/1"),
            json!({"id": PUBLIC_RUN, "run_attempt": 1, "head_sha": PUBLIC, "head_branch": "main", "event": "push",
                   "path": ".github/workflows/product-release-images.yml@refs/heads/main",
                   "head_repository": {"id": 1_390_620_900_u64, "full_name": REPO},
                   "status": "in_progress", "conclusion": null}),
        );
    }

    fn locks(&mut self) {
        let current = toml_of(&self.record, "runtime-preview");
        let base = toml_of(&self.record, "runtime-preview-staged");
        self.set(
            &format!("/contents/.velnor/generator.lock?ref={PUBLIC}"),
            lock_blob(&current),
        );
        self.set(
            &format!("/contents/.velnor/generator.lock?ref={PARENT}"),
            lock_blob(&base),
        );
        self.set(
            &format!("/compare/{SOURCE}...{PARENT}"),
            json!({"status": "ahead", "merge_base_commit": {"sha": SOURCE}}),
        );
        self.set(
            &format!("/compare/{AUTHORITY}...{SOURCE}"),
            json!({"status": "ahead", "merge_base_commit": {"sha": AUTHORITY}}),
        );
    }

    fn jobs(&mut self, run: u64, attempt: u64, jobs: Vec<Value>) {
        let count = jobs.len();
        self.set(
            &format!("/actions/runs/{run}/attempts/{attempt}/jobs?per_page=100&page=1"),
            json!({"total_count": count, "jobs": jobs}),
        );
    }

    fn runs(&mut self) {
        let tag = format!("sdk-preview-{SOURCE}");
        let ci = ".github/workflows/ci.yml";
        let qualification = ".github/workflows/source-qualification.yml";
        self.set(
            "/actions/runs/600/attempts/1",
            run_json(600, 1, "push", "main", ci),
        );
        self.jobs(600, 1, vec![job(6001, 600, 1, "Required")]);
        self.set(
            "/actions/runs/700/attempts/1",
            run_json(700, 1, "workflow_dispatch", "main", qualification),
        );
        self.jobs(700, 1, vec![job(7001, 700, 1, "Source qualification")]);
        for (key, label, run, artifact, qualification_job) in [
            ("linux", "Linux", 800, 11, 8101),
            ("darwin", "Darwin", 900, 13, 9101),
        ] {
            self.set(
                &format!("/actions/runs/{run}/attempts/1"),
                run_json(run, 1, "push", "main", &format!("{SIGNER}@refs/heads/main")),
            );
            self.jobs(
                run,
                1,
                vec![
                    job(run * 10, run, 1, &format!("Build source preview {label}")),
                    job(
                        qualification_job,
                        run,
                        1,
                        &format!("Qualify source preview {label}"),
                    ),
                ],
            );
            self.set(
                &format!("/actions/artifacts/{artifact}"),
                json!({"id": artifact, "expired": false, "name": format!("source-preview-payload-{key}-1"),
                       "digest": format!("sha256:{}", sha(key.as_bytes())),
                       "workflow_run": {"id": run, "head_sha": SOURCE}}),
            );
        }
        for (attempt, name, id) in [
            (1, "Source preview publication / Admit source preview", 5001),
            (2, "Source preview publication / Draft source preview", 5002),
            (
                3,
                "Source preview publication / Attest source preview",
                5003,
            ),
        ] {
            self.set(
                &format!("/actions/runs/500/attempts/{attempt}"),
                run_json(500, attempt, "push", &tag, SIGNER),
            );
            self.jobs(500, attempt, vec![job(id, 500, attempt, name)]);
        }
        let publish = "Source preview publication / Publish source preview";
        // The publisher is attempt 3 of the same run: the attest job and the publish job share it.
        self.jobs(
            500,
            3,
            vec![
                job(
                    5003,
                    500,
                    3,
                    "Source preview publication / Attest source preview",
                ),
                job(7003, 500, 3, publish),
            ],
        );
        self.set("/actions/jobs/7003", json!({"id": 7003, "run_id": 500, "run_attempt": 3, "head_sha": SOURCE, "name": publish, "status": "completed", "conclusion": "success"}));
        self.set(
            &format!("/git/ref/tags/{tag}"),
            json!({"ref": format!("refs/tags/{tag}"), "object": {"type": "tag", "sha": TAG_OID}}),
        );
        self.set(
            &format!("/git/tags/{TAG_OID}"),
            json!({"sha": TAG_OID, "tag": tag, "object": {"type": "commit", "sha": SOURCE}}),
        );
    }

    fn release(&mut self, receipt: &[u8], bundle: &[u8]) {
        let tag = format!("sdk-preview-{SOURCE}");
        let prefix = format!("https://github.com/{REPO}/releases/download/{tag}/");
        let mut assets = Vec::new();
        for (name, data) in [
            ("SDK_PREVIEW_RECEIPT.json", receipt.to_vec()),
            ("SDK_PREVIEW_RECEIPT.json.sigstore.jsonl", bundle.to_vec()),
            ("velnor-actions-x86_64-unknown-linux-gnu", linux_payload()),
            ("velnor-actions-aarch64-apple-darwin", darwin_payload()),
        ] {
            assets.push(json!({"name": name, "state": "uploaded", "size": data.len(), "digest": format!("sha256:{}", sha(&data)),
                               "browser_download_url": format!("{prefix}{name}")}));
            self.assets.insert(format!("{prefix}{name}"), data);
        }
        self.set("/releases/9000", json!({"id": 9000, "tag_name": tag, "draft": false, "prerelease": true, "immutable": true, "assets": assets}));
    }
}

/// What `gh attestation verify --format json` prints for a receipt the publication
/// workflow signed. Written out field by field, apart from the code under test.
pub(crate) fn attestation(record: &Value) -> Value {
    let text = |key: &str| record[key].as_str().unwrap_or_default().to_owned();
    let signer = format!(
        "https://github.com/{REPO}/.github/workflows/source-preview-publication.yml@{AUTHORITY}"
    );
    let source = text("source_sha");
    let tag_ref = format!("refs/tags/sdk-preview-{source}");
    json!([{"verificationResult": {
        "signature": {"certificate": {
            "issuer": "https://token.actions.githubusercontent.com",
            "subjectAlternativeName": signer, "buildSignerURI": signer, "buildSignerDigest": AUTHORITY,
            "buildConfigURI": format!("https://github.com/{REPO}/{SIGNER}@{tag_ref}"),
            "buildConfigDigest": source,
            "sourceRepositoryURI": format!("https://github.com/{REPO}"),
            "sourceRepositoryIdentifier": "1390620900",
            "sourceRepositoryOwnerURI": "https://github.com/tailrocks",
            "sourceRepositoryOwnerIdentifier": "78806509",
            "sourceRepositoryDigest": source, "sourceRepositoryRef": tag_ref,
            "buildTrigger": "push", "runnerEnvironment": "github-hosted",
            "runInvocationURI": format!("https://github.com/{REPO}/actions/runs/500/attempts/3"),
        }},
        "statement": {"predicateType": "https://slsa.dev/provenance/v1", "subject": [
            {"name": "SDK_PREVIEW_RECEIPT.json", "digest": {"sha256": text("receipt_sha256")}},
            {"name": text("linux_payload_name"), "digest": {"sha256": text("linux_payload_sha256")}},
            {"name": text("darwin_payload_name"), "digest": {"sha256": text("darwin_payload_sha256")}},
        ]},
    }}])
}

/// The fake network: answers from the world, fails what it does not know, and records
/// every API path asked.
#[derive(Debug)]
pub(crate) struct FakeNet {
    pub world: World,
    pub calls: RefCell<Vec<String>>,
    deadline: Instant,
}

impl FakeNet {
    pub(crate) fn new(world: World) -> Self {
        Self {
            world,
            calls: RefCell::new(Vec::new()),
            deadline: Instant::now() + Duration::from_secs(180),
        }
    }
}

impl Net for FakeNet {
    fn api(&self, path: &str) -> Res<Value> {
        self.calls.borrow_mut().push(path.to_owned());
        self.world
            .api
            .get(path)
            .cloned()
            .ok_or_else(|| CiError::new(format!("fake: no {path}")))
    }

    fn asset(&self, url: &str, maximum: usize) -> Res<Vec<u8>> {
        self.calls.borrow_mut().push(url.to_owned());
        let body = self
            .world
            .assets
            .get(url)
            .cloned()
            .ok_or_else(|| CiError::new(format!("fake: no {url}")))?;
        if body.len() > maximum {
            return Err(CiError::new("fake: too large"));
        }
        Ok(body)
    }

    fn deadline(&self) -> Instant {
        self.deadline
    }
}

/// Answers every attestation with the world's `gh` output, after the checks `gh`
/// itself would not make.
#[derive(Debug)]
pub(crate) struct FakeAttestor {
    pub output: Value,
    pub seen: RefCell<Vec<(Subject, Authority)>>,
}

impl Attestor for FakeAttestor {
    fn verify(
        &self,
        _receipt: &[u8],
        _bundle: &[u8],
        subject: &Subject,
        authority: &Authority,
        _deadline: Instant,
    ) -> Res<Value> {
        self.seen
            .borrow_mut()
            .push((subject.clone(), authority.clone()));
        velnor_ci::preview::attest::check_verified(&self.output, subject, authority)?;
        Ok(self.output.clone())
    }
}
