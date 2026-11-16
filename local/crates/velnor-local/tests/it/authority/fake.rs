// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;
use std::future::Future;
use std::io;
use std::sync::Mutex;
use std::time::Duration;

use bytes::Bytes;
use http_body_util::{BodyExt, Full};
use hyper::Request;
use serde_json::{Value, json};
use velnor_local::authority::{AGENTS_PATH, RUNS_PATH};
use velnor_local::io::Body;
use velnor_local::io::authority::{Authority, Backend};
use velnor_local::io::pair::{MintError, Minted};
use velnor_local::side::Side;

use crate::support::temp_dir;

pub(super) const TOKEN: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
pub(super) const SHA: &str = "9f3c1e2";

#[derive(Debug, Default)]
pub(super) struct FakeRunners {
    pub(super) runs: Mutex<BTreeMap<(&'static str, String), Value>>,
    pub(super) agents: Mutex<Vec<(Side, u64, String)>>,
    pub(super) down: Mutex<Vec<Side>>,
    pub(super) stubborn: bool,
    pub(super) mint_fails: bool,
    pub(super) mint_stalls: bool,
    pub(super) minted: Mutex<u32>,
}

impl FakeRunners {
    pub(super) fn create(&self, side: Side, id: u64, event: &str, git_ref: &str, sha: &str) {
        self.create_file(side, id, "ci.yml", event, git_ref, sha);
    }

    pub(super) fn create_file(
        &self,
        side: Side,
        id: u64,
        file: &str,
        event: &str,
        git_ref: &str,
        sha: &str,
    ) {
        let name = format!(".github/workflows/{file}");
        let run =
            json!({"id": id, "fileName": name, "eventName": event, "ref": git_ref, "sha": sha});
        let job = json!({"requestId": id, "jobId": "j", "name": "ci", "workflowIdentifier": "ci",
            "workflowname": "ci", "attempt": 1, "runid": id});
        let mut runs = self.runs.lock().unwrap_or_else(|e| unreachable!("{e}"));
        runs.insert((side.name(), format!("run{id}")), run);
        runs.insert((side.name(), format!("jobs{id}")), json!([job]));
    }

    pub(super) fn side_down(&self, side: Side) {
        self.down
            .lock()
            .unwrap_or_else(|e| unreachable!("{e}"))
            .push(side);
    }

    fn answer(&self, side: Side, path: &str) -> io::Result<Option<Vec<u8>>> {
        if self
            .down
            .lock()
            .unwrap_or_else(|e| unreachable!("{e}"))
            .contains(&side)
        {
            return Ok(None);
        }
        let runs = self.runs.lock().unwrap_or_else(|e| unreachable!("{e}"));
        let value = if path == RUNS_PATH {
            let all: Vec<&Value> = runs
                .iter()
                .filter(|((owner, name), _)| *owner == side.name() && name.starts_with("run"))
                .map(|(_, run)| run)
                .collect();
            json!(all)
        } else if path == AGENTS_PATH {
            let agents = self.agents.lock().unwrap_or_else(|e| unreachable!("{e}"));
            let listed: Vec<Value> = agents
                .iter()
                .filter(|(owner, _, _)| *owner == side)
                .map(|(_, id, name)| json!({"id": id, "name": name}))
                .collect();
            json!({"value": listed, "count": listed.len()})
        } else if let Some(id) = path.strip_prefix("/_apis/v1/Message?runid=") {
            runs.get(&(side.name(), format!("jobs{id}")))
                .cloned()
                .unwrap_or_else(|| json!([]))
        } else {
            return Err(io::Error::other(format!("{path} answered 404")));
        };
        Ok(Some(value.to_string().into_bytes()))
    }
}

impl Backend for &FakeRunners {
    fn get(
        &self,
        side: Side,
        path: &str,
    ) -> impl Future<Output = io::Result<Option<Vec<u8>>>> + Send {
        std::future::ready(self.answer(side, path))
    }

    fn delete(&self, side: Side, path: &str) -> impl Future<Output = io::Result<()>> + Send {
        let id: u64 = path
            .rsplit('/')
            .next()
            .and_then(|id| id.parse().ok())
            .unwrap_or(0);
        if !self.stubborn {
            let mut agents = self.agents.lock().unwrap_or_else(|e| unreachable!("{e}"));
            agents.retain(|(owner, agent, _)| !(*owner == side && *agent == id));
        }
        std::future::ready(Ok(()))
    }

    fn mint(
        &self,
        side: Side,
        name: Option<&str>,
    ) -> impl Future<Output = Result<Option<Minted>, MintError>> + Send {
        let name = name.unwrap_or("velnor-x-jit-1").to_owned();
        async move {
            *self.minted.lock().unwrap_or_else(|e| unreachable!("{e}")) += 1;
            if self.mint_stalls {
                std::future::pending::<()>().await;
            }
            if self.mint_fails {
                self.agents
                    .lock()
                    .unwrap_or_else(|e| unreachable!("{e}"))
                    .push((side, 9, name.clone()));
                return Err(MintError {
                    name,
                    source: io::Error::other("the registration output was truncated"),
                });
            }
            let down = self
                .down
                .lock()
                .unwrap_or_else(|e| unreachable!("{e}"))
                .contains(&side);
            Ok((!down).then(|| Minted {
                name,
                jit: "e30=".to_owned(),
                took: Duration::from_millis(1500),
            }))
        }
    }
}

pub(super) async fn authority<'a>(
    name: &str,
    runners: &'a FakeRunners,
) -> Authority<&'a FakeRunners> {
    let dir = temp_dir(name);
    Authority::open(runners, TOKEN.to_owned(), &dir.join("authority"))
        .await
        .unwrap_or_else(|e| unreachable!("{e}"))
}

pub(super) async fn ask(
    authority: &Authority<&FakeRunners>,
    method: &str,
    path: &str,
    token: Option<&str>,
    body: Value,
) -> (u16, Value) {
    let mut request = Request::builder().method(method).uri(path);
    if let Some(token) = token {
        request = request.header("authorization", format!("Bearer {token}"));
    }
    let body: Body = Full::new(Bytes::from(body.to_string()))
        .map_err(|never| match never {})
        .boxed_unsync();
    let response = authority
        .handle(request.body(body).unwrap_or_else(|e| unreachable!("{e}")))
        .await;
    let status = response.status().as_u16();
    let bytes = response
        .into_body()
        .collect()
        .await
        .map_or_else(|_| Bytes::new(), http_body_util::Collected::to_bytes);
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

pub(super) fn entry(id: &str, side: &str, event: &str, git_ref: &str) -> Value {
    json!({"id": id, "side": side, "event": event, "ref": git_ref, "sha": SHA, "workflow": null})
}

pub(super) async fn post(authority: &Authority<&FakeRunners>, value: Value) -> u16 {
    ask(authority, "POST", "/ledger", Some(TOKEN), value)
        .await
        .0
}

pub(super) async fn run_facts(
    authority: &Authority<&FakeRunners>,
    side: &str,
    id: u64,
) -> (u16, Value) {
    ask(
        authority,
        "GET",
        &format!("/runs/{side}/{id}/1"),
        Some(TOKEN),
        Value::Null,
    )
    .await
}
