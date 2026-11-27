// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, VecDeque};
use std::fmt::Write as _;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Instant;

use anyhow::{Result, anyhow};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use super::support::{RecordedRequest, Reply, TestServer};

pub(crate) const TOKEN: &str = "fake-results-token";

const SERVICE: &str = "/twirp/github.actions.results.api.v1.ArtifactService";
const BLOB: &str = "/blob";

#[derive(Debug, Clone)]
pub(crate) struct Stored {
    pub(crate) id: u64,
    pub(crate) run: String,
    pub(crate) job: String,
    pub(crate) name: String,
    pub(crate) zip: Vec<u8>,
    pub(crate) digest: String,
    raw: Option<String>,
    lost: bool,
    finalized: bool,
}

#[derive(Default)]
struct State {
    artifacts: Vec<Stored>,
    next_id: u64,
    scripted: BTreeMap<&'static str, VecDeque<Reply>>,
    camel_case: bool,
    half_downloads: bool,
    blob_status: Option<u16>,
    put_status: Option<u16>,
}

pub(crate) struct FakeResults {
    server: TestServer,
    state: Arc<Mutex<State>>,
}

fn lock(state: &Mutex<State>) -> Result<MutexGuard<'_, State>> {
    state
        .lock()
        .map_err(|_| anyhow!("fake results state poisoned"))
}

pub(crate) fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .fold(String::new(), |mut text, byte| {
            let _ = write!(text, "{byte:02x}");
            text
        })
}

fn twirp_error(status: u16, code: &str) -> Reply {
    Reply::json(
        status,
        &json!({ "code": code, "msg": "fake results error" }),
    )
}

fn authorized(request: &RecordedRequest) -> bool {
    request.header("authorization") == Some(format!("Bearer {TOKEN}").as_str())
}

fn field<'a>(body: &'a Value, name: &str) -> &'a str {
    body.get(name).and_then(Value::as_str).unwrap_or_default()
}

fn blob_id(request: &RecordedRequest) -> u64 {
    request
        .query("id")
        .and_then(|id| id.parse().ok())
        .unwrap_or_default()
}

impl FakeResults {
    pub(crate) async fn start() -> Result<Self> {
        let server = TestServer::start().await?;
        let state = Arc::new(Mutex::new(State {
            next_id: 100,
            ..State::default()
        }));
        let fake = Self { server, state };
        fake.route("CreateArtifact", Self::create)?;
        fake.route("FinalizeArtifact", Self::finalize)?;
        fake.route("ListArtifacts", Self::list)?;
        fake.route("GetSignedArtifactURL", Self::signed_url)?;
        fake.route("DeleteArtifact", Self::delete)?;
        let state = Arc::clone(&fake.state);
        fake.server.respond("PUT", BLOB, move |request| {
            let Ok(mut state) = lock(&state) else {
                return Reply::status(500);
            };
            let id = blob_id(request);
            let put_status = state.put_status.unwrap_or(201);
            match state.artifacts.iter_mut().find(|stored| stored.id == id) {
                Some(stored) => {
                    stored.zip = request.body().to_vec();
                    Reply::status(put_status)
                }
                None => Reply::status(404),
            }
        })?;
        let state = Arc::clone(&fake.state);
        fake.server.respond("GET", BLOB, move |request| {
            let Ok(state) = lock(&state) else {
                return Reply::status(500);
            };
            let id = blob_id(request);
            match state.artifacts.iter().find(|stored| stored.id == id) {
                Some(stored) if stored.lost => Reply::status(404),
                Some(stored) => {
                    let status = state.blob_status.unwrap_or(200);
                    let reply = match &stored.raw {
                        Some(disposition) => {
                            Reply::bytes(status, "application/octet-stream", stored.zip.clone())
                                .with_header("content-disposition", disposition)
                        }
                        None => Reply::bytes(status, "application/zip", stored.zip.clone()),
                    };
                    if state.half_downloads {
                        reply.half_body()
                    } else {
                        reply
                    }
                }
                None => Reply::status(404),
            }
        })?;
        Ok(fake)
    }

    pub(crate) fn url(&self) -> String {
        self.server.uri()
    }

    fn route(
        &self,
        method: &'static str,
        handler: fn(&mut State, &str, &Value) -> Reply,
    ) -> Result<()> {
        let state = Arc::clone(&self.state);
        let base = self.url();
        self.server
            .respond("POST", &format!("{SERVICE}/{method}"), move |request| {
                if !authorized(request) {
                    return twirp_error(401, "unauthenticated");
                }
                let (Ok(body), Ok(mut state)) = (request.json(), lock(&state)) else {
                    return twirp_error(400, "malformed");
                };
                let reply = match state.scripted.get_mut(method).and_then(VecDeque::pop_front) {
                    Some(reply) => reply,
                    None => handler(&mut state, &base, &body),
                };
                if state.camel_case {
                    reply.camel_cased()
                } else {
                    reply
                }
            })
    }

    fn create(state: &mut State, base: &str, body: &Value) -> Reply {
        let name = field(body, "name");
        if body["version"] != 7 || field(body, "mime_type") != "application/zip" {
            return twirp_error(400, "invalid_argument");
        }
        let run = field(body, "workflow_run_backend_id");
        if state
            .artifacts
            .iter()
            .any(|stored| stored.name == name && stored.run == run)
        {
            return twirp_error(409, "already_exists");
        }
        state.next_id += 1;
        let id = state.next_id;
        state.artifacts.push(Stored {
            id,
            run: field(body, "workflow_run_backend_id").to_owned(),
            job: field(body, "workflow_job_run_backend_id").to_owned(),
            name: name.to_owned(),
            zip: Vec::new(),
            digest: String::new(),
            raw: None,
            lost: false,
            finalized: false,
        });
        Reply::json(
            200,
            &json!({ "ok": true, "signed_upload_url": format!("{base}{BLOB}?id={id}") }),
        )
    }

    fn finalize(state: &mut State, _base: &str, body: &Value) -> Reply {
        let name = field(body, "name");
        let Some(stored) = state
            .artifacts
            .iter_mut()
            .find(|stored| stored.name == name && !stored.finalized)
        else {
            return twirp_error(404, "not_found");
        };
        let digest = format!("sha256:{}", sha256_hex(&stored.zip));
        if field(body, "size") != stored.zip.len().to_string() || field(body, "hash") != digest {
            return twirp_error(400, "invalid_argument");
        }
        stored.finalized = true;
        stored.digest = digest;
        Reply::json(
            200,
            &json!({ "ok": true, "artifact_id": stored.id.to_string() }),
        )
    }

    fn list(state: &mut State, _base: &str, body: &Value) -> Reply {
        let filter = body.get("name_filter").and_then(Value::as_str);
        let run = field(body, "workflow_run_backend_id");
        let artifacts: Vec<Value> = state
            .artifacts
            .iter()
            .filter(|stored| {
                stored.finalized
                    && stored.run == run
                    && filter.is_none_or(|name| stored.name == name)
            })
            .map(|stored| {
                json!({
                    "workflow_run_backend_id": stored.run,
                    "workflow_job_run_backend_id": stored.job,
                    "database_id": stored.id.to_string(),
                    "name": stored.name,
                    "size": stored.zip.len().to_string(),
                    "digest": stored.digest,
                })
            })
            .collect();
        Reply::json(200, &json!({ "artifacts": artifacts }))
    }

    fn signed_url(state: &mut State, base: &str, body: &Value) -> Reply {
        let found = state.artifacts.iter().find(|stored| {
            stored.finalized
                && stored.name == field(body, "name")
                && stored.run == field(body, "workflow_run_backend_id")
                && stored.job == field(body, "workflow_job_run_backend_id")
        });
        match found {
            Some(stored) => Reply::json(
                200,
                &json!({ "signed_url": format!("{base}{BLOB}?id={}", stored.id) }),
            ),
            None => twirp_error(404, "not_found"),
        }
    }

    fn delete(state: &mut State, _base: &str, body: &Value) -> Reply {
        let position = state.artifacts.iter().position(|stored| {
            stored.finalized
                && stored.name == field(body, "name")
                && stored.run == field(body, "workflow_run_backend_id")
                && stored.job == field(body, "workflow_job_run_backend_id")
        });
        match position {
            Some(index) => {
                let removed = state.artifacts.remove(index);
                Reply::json(
                    200,
                    &json!({ "ok": true, "artifact_id": removed.id.to_string() }),
                )
            }
            None => twirp_error(404, "not_found"),
        }
    }

    pub(crate) fn seed(&self, name: &str, run: &str, job: &str, zip: Vec<u8>) -> Result<u64> {
        let mut state = lock(&self.state)?;
        state.next_id += 1;
        let id = state.next_id;
        let digest = format!("sha256:{}", sha256_hex(&zip));
        state.artifacts.push(Stored {
            id,
            run: run.to_owned(),
            job: job.to_owned(),
            name: name.to_owned(),
            zip,
            digest,
            raw: None,
            lost: false,
            finalized: true,
        });
        Ok(id)
    }

    pub(crate) fn seed_raw(
        &self,
        name: &str,
        run: &str,
        job: &str,
        bytes: Vec<u8>,
        disposition: &str,
    ) -> Result<u64> {
        let id = self.seed(name, run, job, bytes)?;
        for stored in &mut lock(&self.state)?.artifacts {
            if stored.id == id {
                stored.raw = Some(disposition.to_owned());
            }
        }
        Ok(id)
    }

    pub(crate) fn script(&self, method: &'static str, replies: Vec<Reply>) -> Result<()> {
        lock(&self.state)?
            .scripted
            .entry(method)
            .or_default()
            .extend(replies);
        Ok(())
    }

    pub(crate) fn answer_blobs_with(&self, status: u16) -> Result<()> {
        lock(&self.state)?.blob_status = Some(status);
        Ok(())
    }

    pub(crate) fn answer_uploads_with(&self, status: u16) -> Result<()> {
        lock(&self.state)?.put_status = Some(status);
        Ok(())
    }

    pub(crate) fn stall_downloads_halfway(&self) -> Result<()> {
        lock(&self.state)?.half_downloads = true;
        Ok(())
    }

    pub(crate) fn respond_in_camel_case(&self) -> Result<()> {
        lock(&self.state)?.camel_case = true;
        Ok(())
    }

    pub(crate) fn lose_blob(&self, name: &str) -> Result<()> {
        for stored in &mut lock(&self.state)?.artifacts {
            if stored.name == name {
                stored.lost = true;
            }
        }
        Ok(())
    }

    pub(crate) fn set_digest(&self, name: &str, digest: &str) -> Result<()> {
        for stored in &mut lock(&self.state)?.artifacts {
            if stored.name == name {
                digest.clone_into(&mut stored.digest);
            }
        }
        Ok(())
    }

    pub(crate) fn artifacts(&self) -> Result<Vec<Stored>> {
        Ok(lock(&self.state)?
            .artifacts
            .iter()
            .filter(|stored| stored.finalized)
            .cloned()
            .collect())
    }

    pub(crate) fn blob_gets(&self) -> Result<usize> {
        Ok(self.server.requests_to("GET", BLOB)?.len())
    }

    pub(crate) fn arrivals(&self, method: &str) -> Result<Vec<Instant>> {
        Ok(self
            .server
            .requests_to("POST", &format!("{SERVICE}/{method}"))?
            .iter()
            .map(RecordedRequest::arrived)
            .collect())
    }

    pub(crate) fn calls(&self, method: &str) -> Result<Vec<Value>> {
        self.server
            .requests_to("POST", &format!("{SERVICE}/{method}"))?
            .iter()
            .map(RecordedRequest::json)
            .collect()
    }
}
