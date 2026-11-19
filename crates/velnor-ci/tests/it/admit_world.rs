// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

//! A complete, valid GitHub as `admit-preview` sees it: the owner's tag push, the tag
//! rulesets, the annotated tag, the qualification runs and both producer runs with their
//! payload artifacts. For the tests of the helper and its parity with the legacy script.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::time::{Duration, Instant};

use serde_json::{Value, json};
use velnor_ci::error::CiError;
use velnor_ci::preview::authority::Client;
use velnor_ci::preview::json::{Res, canonical};
use velnor_ci::preview::net::Net;

use crate::admit_zip::zip_of;
use crate::preview_world::{
    R, REPO, SIGNER, SOURCE, TAG_OID, darwin_payload, linux_payload, lock_blob, sha,
};

pub(crate) const MAIN: &str = "5555555555555555555555555555555555555555";
pub(crate) const RUN: u64 = 5000;
/// The neutral owner of the fixtures. The helper and the legacy script both take it from the run.
pub(crate) const OWNER: &str = "octo-owner";
pub(crate) const USER: &str = "/users/octo-owner";
/// The neutral account id of the fixtures' release owner, which a deployment configures.
pub(crate) const OWNER_ID: u64 = 4242;
const PUBLICATION: &str = ".github/workflows/source-preview-publication.yml";
const AUTHORITY: &str = "7777777777777777777777777777777777777777";

pub(crate) fn tag() -> String {
    format!("sdk-preview-{SOURCE}")
}

fn job(id: u64, run: u64, name: &str) -> Value {
    json!({"id": id, "run_id": run, "run_attempt": 1, "head_sha": SOURCE, "name": name,
           "status": "completed", "conclusion": "success"})
}

fn run_json(id: u64, event: &str, branch: &str, path: &str) -> Value {
    json!({"id": id, "run_attempt": 1, "event": event, "head_sha": SOURCE, "head_branch": branch,
           "path": path, "status": "completed", "conclusion": "success",
           "head_repository": {"id": 1_390_620_900_u64, "full_name": REPO}})
}

#[derive(Debug, Clone)]
pub(crate) struct AdmitWorld {
    pub api: BTreeMap<String, Value>,
    pub zips: BTreeMap<u64, Vec<u8>>,
    pub vars: BTreeMap<String, String>,
    /// The login the legacy script is pinned to; the helper reads it from `vars`.
    pub owner: String,
}

impl AdmitWorld {
    pub(crate) fn path(suffix: &str) -> String {
        if suffix == USER {
            suffix.to_owned()
        } else {
            format!("{R}{suffix}")
        }
    }

    pub(crate) fn set(&mut self, suffix: &str, value: Value) {
        self.api.insert(Self::path(suffix), value);
    }

    pub(crate) fn edit(&mut self, suffix: &str, change: impl FnOnce(&mut Value)) {
        change(
            self.api
                .get_mut(&Self::path(suffix))
                .unwrap_or_else(|| unreachable!("{suffix}")),
        );
    }

    /// Rewrite the tag annotation with `change`, keeping it canonical.
    pub(crate) fn annotate(&mut self, change: impl FnOnce(&mut Value)) {
        let at = format!("/git/tags/{TAG_OID}");
        self.edit(&at, |tag| {
            let mut record: Value =
                serde_json::from_str(tag["message"].as_str().unwrap_or("")).unwrap_or(Value::Null);
            change(&mut record);
            tag["message"] = json!(String::from_utf8_lossy(&canonical(&record)) + "\n");
        });
    }

    /// Replace the ZIP of `key` and bring the artifact listing and the annotation along.
    pub(crate) fn rezip(&mut self, key: &str, zip: Vec<u8>) {
        let (id, run) = if key == "linux" { (11, 800) } else { (13, 900) };
        let digest = format!("sha256:{}", sha(&zip));
        let size = zip.len();
        self.edit(
            &format!("/actions/runs/{run}/artifacts?per_page=100"),
            |v| {
                v["artifacts"][0]["digest"] = json!(digest);
                v["artifacts"][0]["size_in_bytes"] = json!(size);
            },
        );
        self.annotate(|r| r["targets"][key]["payload_archive_sha256"] = json!(sha(&zip)));
        self.zips.insert(id, zip);
    }

    pub(crate) fn new() -> Self {
        let mut world = Self {
            api: BTreeMap::new(),
            zips: BTreeMap::new(),
            vars: BTreeMap::new(),
            owner: OWNER.to_owned(),
        };
        world.environment();
        world.account();
        world.qualification();
        let archives = world.producers();
        world.annotation(&archives);
        world
    }

    fn environment(&mut self) {
        let tag = tag();
        let publication = format!("{REPO}/{PUBLICATION}@{AUTHORITY}");
        for (key, value) in [
            ("GH_TOKEN", "x".to_owned()),
            ("GITHUB_SHA", SOURCE.to_owned()),
            ("GITHUB_REPOSITORY", REPO.to_owned()),
            ("GITHUB_EVENT_NAME", "push".to_owned()),
            ("GITHUB_REF", format!("refs/tags/{tag}")),
            (
                "GITHUB_WORKFLOW_REF",
                format!("{REPO}/{SIGNER}@refs/tags/{tag}"),
            ),
            ("GITHUB_WORKFLOW_SHA", SOURCE.to_owned()),
            ("RELEASE_OWNER_ID", OWNER_ID.to_string()),
            ("GITHUB_ACTOR_ID", OWNER_ID.to_string()),
            ("GITHUB_TRIGGERING_ACTOR", OWNER.to_owned()),
            ("GITHUB_RUN_ID", RUN.to_string()),
            ("GITHUB_RUN_ATTEMPT", "1".to_owned()),
            ("AUTHORITY_REPOSITORY", REPO.to_owned()),
            ("AUTHORITY_PATH", PUBLICATION.to_owned()),
            ("AUTHORITY_REF", publication),
            ("AUTHORITY_SHA", AUTHORITY.to_owned()),
        ] {
            self.vars.insert(key.to_owned(), value);
        }
    }

    fn account(&mut self) {
        let tag = tag();
        self.set("", json!({"id": 1_390_620_900_u64, "full_name": REPO, "default_branch": "main", "owner": {"id": 78_806_509}}));
        let mut cargo = lock_blob("[workspace.package]\nversion = \"0.1.82\"\n");
        cargo["path"] = json!("Cargo.toml");
        self.set(&format!("/contents/Cargo.toml?ref={SOURCE}"), cargo);
        self.set(USER, json!({"id": OWNER_ID, "login": OWNER}));
        self.set(
            &format!("/actions/runs/{RUN}"),
            json!({"id": RUN, "run_attempt": 1, "head_sha": SOURCE, "head_branch": tag, "event": "push",
                   "path": format!("{SIGNER}@refs/tags/{tag}"), "created_at": "2026-11-18T12:10:00Z",
                   "head_repository": {"id": 1_390_620_900_u64, "full_name": REPO}}),
        );
        self.set(
            "/rulesets/26696396",
            json!({"id": 26_696_396, "target": "tag", "enforcement": "active",
            "bypass_actors": [{"actor_id": OWNER_ID, "actor_type": "User", "bypass_mode": "always"}],
            "conditions": {"ref_name": {"include": ["refs/tags/sdk-preview-*"], "exclude": []}},
            "rules": [{"type": "creation"}]}),
        );
        self.set(
            "/rulesets/24397132",
            json!({"id": 24_397_132, "target": "tag", "enforcement": "active",
            "bypass_actors": [], "conditions": {"ref_name": {"include": ["~ALL"], "exclude": []}},
            "rules": [{"type": "update"}, {"type": "deletion"}, {"type": "non_fast_forward"}]}),
        );
        self.set(
            &format!("/git/ref/tags/{tag}"),
            json!({"ref": format!("refs/tags/{tag}"), "object": {"type": "tag", "sha": TAG_OID}}),
        );
        self.set(
            "/git/ref/heads/main",
            json!({"object": {"type": "commit", "sha": MAIN}}),
        );
        self.set(
            &format!("/compare/{SOURCE}...{MAIN}"),
            json!({"status": "ahead", "merge_base_commit": {"sha": SOURCE}}),
        );
    }

    fn qualification(&mut self) {
        for (file, event, id, names) in [
            ("ci.yml", "push", 600, vec!["Required verdict"]),
            (
                "source-qualification.yml",
                "workflow_dispatch",
                700,
                vec![
                    "Source qualification ordinary harness",
                    "Source qualification keep harness",
                    "Source qualification generation and repository checks",
                    "Source qualification",
                ],
            ),
        ] {
            let path = format!(".github/workflows/{file}");
            self.set(
                &format!("/actions/workflows/{file}/runs?head_sha={SOURCE}&event={event}&status=success&per_page=20"),
                json!({"workflow_runs": [run_json(id, event, "main", &path)]}),
            );
            let jobs: Vec<Value> = names
                .iter()
                .zip(1..)
                .map(|(n, i)| job(id * 10 + i, id, n))
                .collect();
            self.set(
                &format!("/actions/runs/{id}/attempts/1/jobs?per_page=100"),
                json!({"total_count": jobs.len(), "jobs": jobs}),
            );
        }
    }

    /// Both producer runs, and the sha256 of each payload ZIP.
    fn producers(&mut self) -> BTreeMap<&'static str, String> {
        let mut archives = BTreeMap::new();
        for (key, label, run, artifact, qualify, payload, name) in [
            (
                "linux",
                "Linux",
                800,
                11,
                8101,
                linux_payload(),
                "velnor-actions-x86_64-unknown-linux-gnu",
            ),
            (
                "darwin",
                "Darwin",
                900,
                13,
                9101,
                darwin_payload(),
                "velnor-actions-aarch64-apple-darwin",
            ),
        ] {
            let zip = zip_of(&[(name, &payload)], 0, 0o100_644);
            archives.insert(key, sha(&zip));
            self.set(
                &format!("/actions/runs/{run}/attempts/1"),
                run_json(run, "push", "main", &format!("{SIGNER}@refs/heads/main")),
            );
            self.set(
                &format!("/actions/runs/{run}/attempts/1/jobs?per_page=100"),
                json!({"total_count": 2, "jobs": [
                job(run * 10, run, &format!("Build source preview {label}")),
                job(qualify, run, &format!("Qualify source preview {label}"))]}),
            );
            self.set(&format!("/actions/runs/{run}/artifacts?per_page=100"), json!({"total_count": 1, "artifacts": [
                {"id": artifact, "name": format!("source-preview-payload-{key}-1"), "expired": false,
                 "size_in_bytes": zip.len(), "digest": format!("sha256:{}", sha(&zip)),
                 "workflow_run": {"id": run, "head_sha": SOURCE}}]}));
            self.zips.insert(artifact, zip);
        }
        archives
    }

    fn annotation(&mut self, archives: &BTreeMap<&'static str, String>) {
        let tag = tag();
        let target = |run: u64, artifact: u64, job: u64, key: &str| {
            json!({"producer_run_id": run, "producer_run_attempt": 1,
            "payload_artifact_id": artifact, "payload_archive_sha256": archives[key], "qualification_job_id": job})
        };
        let record = json!({"schema": 2, "repository": REPO, "artifact": "generator", "source_sha": SOURCE, "version": "0.1.82",
            "targets": {"linux": target(800, 11, 8101, "linux"), "darwin": target(900, 13, 9101, "darwin")},
            "immutability": {"enabled": true, "checked_at": "2026-11-18T12:00:00Z"}});
        self.set(&format!("/git/tags/{TAG_OID}"), json!({"sha": TAG_OID, "tag": tag,
            "object": {"type": "commit", "sha": SOURCE, "url": format!("https://api.github.com/repos/{REPO}/git/commits/{SOURCE}")},
            "message": String::from_utf8_lossy(&canonical(&record)) + "\n",
            "tagger": {"date": "2026-11-18T12:00:30Z"}}));
    }

    /// The fake network for the helper.
    pub(crate) fn client(&self) -> FakeClient {
        FakeClient {
            world: RefCell::new(self.clone()),
            calls: RefCell::new(Vec::new()),
            deadline: Instant::now() + Duration::from_secs(120),
        }
    }
}

/// Answers from the world and records every request. A value of the form
/// `{"__seq": [a, b]}` answers `a` once and then `b`.
#[derive(Debug)]
pub(crate) struct FakeClient {
    world: RefCell<AdmitWorld>,
    pub calls: RefCell<Vec<String>>,
    deadline: Instant,
}

impl Net for FakeClient {
    fn api(&self, path: &str) -> Res<Value> {
        self.calls.borrow_mut().push(path.to_owned());
        let mut world = self.world.borrow_mut();
        let value = world
            .api
            .get_mut(path)
            .ok_or_else(|| CiError::new(format!("fake: no {path}")))?;
        if let Some(items) = value.get_mut("__seq").and_then(Value::as_array_mut) {
            let item = items.first().cloned().unwrap_or(Value::Null);
            if items.len() > 1 {
                items.remove(0);
            }
            return Ok(item);
        }
        Ok(value.clone())
    }

    fn asset(&self, url: &str, _maximum: usize) -> Res<Vec<u8>> {
        Err(CiError::new(format!("fake: no asset {url}")))
    }

    fn deadline(&self) -> Instant {
        self.deadline
    }
}

impl Client for FakeClient {
    fn send(&self, _method: &str, suffix: &str, _body: &[u8], _allowed: &[u16]) -> Res<Value> {
        Err(CiError::new(format!("fake: unexpected write {suffix}")))
    }

    fn upload(&self, _release_id: u64, name: &str, _data: &[u8]) -> Res<Value> {
        Err(CiError::new(format!("fake: unexpected upload {name}")))
    }

    fn artifact_zip(&self, id: u64, _size: u64, _digest: &str) -> Res<Vec<u8>> {
        self.calls
            .borrow_mut()
            .push(format!("{R}/actions/artifacts/{id}/zip"));
        self.world
            .borrow()
            .zips
            .get(&id)
            .cloned()
            .ok_or_else(|| CiError::new("fake: no artifact"))
    }
}
