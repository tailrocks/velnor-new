// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

mod runners;

use std::future::Future;
use std::io;
use std::path::{Path, PathBuf};

use hyper::header::AUTHORIZATION;
use hyper::{Request, Response};
use ring::hmac;
use serde::de::DeserializeOwned;
use serde_json::json;
use tokio::fs;
use tokio::sync::Mutex;

pub use runners::Runners;

use super::pair::{MintError, Minted, write_private};
use super::{Body, json_response, read_small_body};
use crate::authority::{
    AGENTS_PATH, Call, Entry, JitRequest, Posted, RUNS_PATH, admit, agent_id, agent_name, answer,
    call, claim, facts, run_ids,
};
use crate::protocol::{Error, parse};
use crate::side::Side;
use crate::standin::{Job, Run};

pub const LEDGER: &str = "ledger.jsonl";

pub trait Backend: Send + Sync {
    fn get(
        &self,
        side: Side,
        path: &str,
    ) -> impl Future<Output = io::Result<Option<Vec<u8>>>> + Send;
    fn delete(&self, side: Side, path: &str) -> impl Future<Output = io::Result<()>> + Send;
    fn mint(
        &self,
        side: Side,
        name: Option<&str>,
    ) -> impl Future<Output = Result<Option<Minted>, MintError>> + Send;
}

#[derive(Debug)]
struct Ledger {
    path: PathBuf,
    entries: Vec<Entry>,
}

impl Ledger {
    async fn save(&self) -> io::Result<()> {
        let mut text = String::new();
        for entry in &self.entries {
            text.push_str(&json!(entry).to_string());
            text.push('\n');
        }
        write_private(&self.path, &text).await
    }
}

#[derive(Debug)]
pub struct Authority<B> {
    backend: B,
    key: hmac::Key,
    expected: hmac::Tag,
    ledger: Mutex<Ledger>,
}

impl<B: Backend> Authority<B> {
    pub async fn open(backend: B, token: String, dir: &Path) -> io::Result<Self> {
        fs::create_dir_all(dir).await?;
        let path = dir.join(LEDGER);
        let kept = match fs::read_to_string(&path).await {
            Err(error) if error.kind() == io::ErrorKind::NotFound => String::new(),
            read => read?,
        };
        let entries = kept
            .lines()
            .map(|line| serde_json::from_str(line).map_err(io::Error::other))
            .collect::<io::Result<_>>()?;
        let key = hmac::Key::new(hmac::HMAC_SHA256, token.as_bytes());
        let expected = hmac::sign(&key, format!("Bearer {token}").as_bytes());
        Ok(Self {
            backend,
            key,
            expected,
            ledger: Mutex::new(Ledger { path, entries }),
        })
    }

    pub async fn handle(&self, request: Request<Body>) -> Response<Body> {
        if !self.authorized(&request) {
            let refusal = Error::unauthenticated("the authority needs its token as a bearer token");
            return json_response(refusal.status, &refusal.body());
        }
        let method = request.method().as_str().to_owned();
        let path = request.uri().path().to_owned();
        let result = match call(&method, &path) {
            Call::Ledger => self.record(request).await,
            Call::Jit => self.jit(request).await,
            Call::Run { side, id, attempt } => self.run(side, id, attempt).await,
            Call::Agent { side, name } => self.remove(side, name).await,
            Call::Unknown => Err(Error::not_found(format!("{method} {path} is not served"))),
        };
        let response = result.unwrap_or_else(|error| json_response(error.status, &error.body()));
        eprintln!("authority: {method} {path} {}", response.status().as_u16());
        response
    }

    fn authorized(&self, request: &Request<Body>) -> bool {
        request.headers().get(AUTHORIZATION).is_some_and(|value| {
            hmac::verify(&self.key, value.as_bytes(), self.expected.as_ref()).is_ok()
        })
    }

    async fn fetch<T: DeserializeOwned>(&self, side: Side, path: &str) -> Result<T, Error> {
        let bytes = self
            .backend
            .get(side, path)
            .await
            .map_err(|error| Error::upstream(format!("{} runner.server: {error}", side.name())))?
            .ok_or_else(|| Error::not_found(format!("the {} side is not up", side.name())))?;
        parse(&bytes)
            .map_err(|error| Error::upstream(format!("runner.server {path}: {}", error.msg)))
    }

    async fn record(&self, request: Request<Body>) -> Result<Response<Body>, Error> {
        let posted: Posted = parse(&read_small_body(request.into_body()).await?)?;
        let mut ledger = self.ledger.lock().await;
        if ledger.entries.iter().any(|entry| entry.id == posted.id) {
            return Err(Error::precondition(format!(
                "{} is already in the ledger",
                posted.id
            )));
        }
        let runs: Vec<Run> = self.fetch(posted.side, RUNS_PATH).await?;
        let entry = posted.entry(run_ids(&runs))?;
        ledger.entries.push(entry.clone());
        if let Err(error) = ledger.save().await {
            ledger.entries.pop();
            return Err(Error::internal(error));
        }
        let reply =
            json!({"id": entry.id, "side": entry.side.name(), "existing": entry.existing.len()});
        Ok(json_response(200, &reply))
    }

    async fn run(&self, side: Side, id: u64, attempt: u32) -> Result<Response<Body>, Error> {
        let runs: Vec<Run> = self.fetch(side, RUNS_PATH).await?;
        let missing =
            || Error::not_found(format!("{} has no run {id} attempt {attempt}", side.name()));
        if !runs.iter().any(|run| run.id == id) {
            return Err(missing());
        }
        let jobs: Vec<Job> = self
            .fetch(side, &format!("/_apis/v1/Message?runid={id}"))
            .await?;
        let found = facts(&runs, &jobs, id, attempt).ok_or_else(missing)?;
        let mut ledger = self.ledger.lock().await;
        let admitted = admit(&ledger.entries, side, &found).cloned();
        if let Ok(entry) = &admitted {
            let before = ledger.entries.clone();
            if claim(&mut ledger.entries, &entry.id, &found)
                && let Err(error) = ledger.save().await
            {
                ledger.entries = before;
                return Err(Error::internal(error));
            }
        }
        let verdict = admitted.as_ref().map_err(|refusal| *refusal);
        Ok(json_response(200, &answer(side, &found, verdict)))
    }

    async fn jit(&self, request: Request<Body>) -> Result<Response<Body>, Error> {
        let asked: JitRequest = parse(&read_small_body(request.into_body()).await?)?;
        if asked.name.as_deref().is_some_and(|name| !agent_name(name)) {
            return Err(Error::invalid("the agent name is not valid".to_owned()));
        }
        let minted = match self.backend.mint(asked.side, asked.name.as_deref()).await {
            Ok(minted) => minted,
            Err(error) => {
                let _removed = self.remove(asked.side, &error.name).await;
                return Err(Error::upstream(format!("minting a JIT: {error}")));
            }
        }
        .ok_or_else(|| Error::not_found(format!("the {} side is not up", asked.side.name())))?;
        let reply =
            json!({"name": minted.name, "jit": minted.jit, "seconds": minted.took.as_secs_f64()});
        Ok(json_response(200, &reply))
    }

    async fn remove(&self, side: Side, name: &str) -> Result<Response<Body>, Error> {
        let listed = |bytes: Vec<u8>| agent_id(&bytes, name);
        let listing = |error: io::Error| Error::upstream(format!("runner.server: {error}"));
        let before = self.backend.get(side, AGENTS_PATH).await.map_err(listing)?;
        let before = before
            .ok_or_else(|| Error::not_found(format!("the {} side is not up", side.name())))?;
        let Some(id) = listed(before) else {
            return Ok(json_response(200, &json!({"name": name, "deleted": false})));
        };
        self.backend
            .delete(side, &format!("{AGENTS_PATH}/{id}"))
            .await
            .map_err(listing)?;
        let after = self.backend.get(side, AGENTS_PATH).await.map_err(listing)?;
        if after.and_then(listed).is_some() {
            return Err(Error::upstream(format!(
                "{name} is still registered after its deletion"
            )));
        }
        Ok(json_response(200, &json!({"name": name, "deleted": true})))
    }
}
