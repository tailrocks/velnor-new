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
use velnor_local::authority::{AGENTS_PATH, Call, Entry, Facts, RUNS_PATH, Refusal, admit, call};
use velnor_local::io::Body;
use velnor_local::io::authority::{Authority, Backend};
use velnor_local::io::pair::Minted;
use velnor_local::side::Side;

use crate::support::temp_dir;

const TOKEN: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
const SHA: &str = "9f3c1e2";

#[derive(Debug, Default)]
struct FakeRunners {
    runs: Mutex<BTreeMap<(&'static str, String), Value>>,
    agents: Mutex<Vec<(Side, u64, String)>>,
    down: Mutex<Vec<Side>>,
    stubborn: bool,
}

impl FakeRunners {
    fn create(&self, side: Side, id: u64, event: &str, git_ref: &str, sha: &str) {
        let run = json!({"id": id, "fileName": ".github/workflows/ci.yml", "eventName": event, "ref": git_ref, "sha": sha});
        let job = json!({"requestId": id, "jobId": "j", "name": "ci", "workflowIdentifier": "ci",
            "workflowname": "ci", "attempt": 1, "runid": id});
        let mut runs = self.runs.lock().unwrap_or_else(|e| unreachable!("{e}"));
        runs.insert((side.name(), format!("run{id}")), run);
        runs.insert((side.name(), format!("jobs{id}")), json!([job]));
    }

    fn side_down(&self, side: Side) {
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

    fn mint(&self, side: Side) -> impl Future<Output = io::Result<Option<Minted>>> + Send {
        let down = self
            .down
            .lock()
            .unwrap_or_else(|e| unreachable!("{e}"))
            .contains(&side);
        std::future::ready(Ok((!down).then(|| Minted {
            name: "velnor-x-jit-1".to_owned(),
            jit: "e30=".to_owned(),
            took: Duration::from_millis(1500),
        })))
    }
}

async fn authority<'a>(name: &str, runners: &'a FakeRunners) -> Authority<&'a FakeRunners> {
    let dir = temp_dir(name);
    Authority::open(runners, TOKEN.to_owned(), &dir.join("authority"))
        .await
        .unwrap_or_else(|e| unreachable!("{e}"))
}

async fn ask(
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

fn entry(id: &str, side: &str, event: &str, git_ref: &str) -> Value {
    json!({"id": id, "side": side, "event": event, "ref": git_ref, "sha": SHA, "workflow": null})
}

async fn post(authority: &Authority<&FakeRunners>, value: Value) -> u16 {
    ask(authority, "POST", "/ledger", Some(TOKEN), value)
        .await
        .0
}

async fn run_facts(authority: &Authority<&FakeRunners>, side: &str, id: u64) -> (u16, Value) {
    ask(
        authority,
        "GET",
        &format!("/runs/{side}/{id}/1"),
        Some(TOKEN),
        Value::Null,
    )
    .await
}

#[tokio::test]
async fn authority_answers_401_to_every_call_without_its_token() {
    let runners = FakeRunners::default();
    let authority = authority("authority-401", &runners).await;
    let wrong = format!("{}0", &TOKEN[..63]);
    for (method, path) in [
        ("GET", "/runs/trusted/1/1"),
        ("POST", "/ledger"),
        ("POST", "/jit"),
        ("DELETE", "/agents/trusted/a"),
        ("GET", "/anything"),
    ] {
        for token in [None, Some(wrong.as_str()), Some("")] {
            let (status, _) = ask(&authority, method, path, token, json!({})).await;
            assert_eq!(status, 401, "{method} {path} {token:?}");
        }
    }
}

#[tokio::test]
async fn authority_returns_the_run_with_the_ledger_entry_that_dispatched_it() {
    let runners = FakeRunners::default();
    let authority = authority("authority-match", &runners).await;
    assert_eq!(
        post(
            &authority,
            entry("1-push", "trusted", "push", "refs/heads/main")
        )
        .await,
        200
    );
    runners.create(Side::Trusted, 1, "push", "refs/heads/main", SHA);
    let (status, run) = run_facts(&authority, "trusted", 1).await;
    assert_eq!(status, 200, "{run}");
    assert_eq!(run["ledger"], "1-push", "{run}");
    assert_eq!(run["jobs"], json!(["ci"]));
    assert_eq!(run["refusal"], Value::Null);
    let (status, _) = run_facts(&authority, "untrusted", 1).await;
    assert_eq!(status, 404);
    let (status, _) = run_facts(&authority, "trusted", 2).await;
    assert_eq!(status, 404);
}

#[tokio::test]
async fn authority_refuses_a_run_that_matches_no_dispatched_event_as_not_dispatched() {
    let runners = FakeRunners::default();
    let authority = authority("authority-not-dispatched", &runners).await;
    assert_eq!(
        post(
            &authority,
            entry("1-push", "trusted", "push", "refs/heads/main")
        )
        .await,
        200
    );
    runners.create(Side::Trusted, 1, "push", "refs/heads/main", SHA);
    runners.create(Side::Trusted, 2, "push", "refs/heads/other", SHA);
    let (_, run) = run_facts(&authority, "trusted", 2).await;
    assert_eq!(run["ledger"], Value::Null, "{run}");
    assert_eq!(run["refusal"], "not_dispatched", "{run}");
}

#[tokio::test]
async fn authority_refuses_a_ledger_entry_on_the_wrong_side_as_wrong_side() {
    let runners = FakeRunners::default();
    let authority = authority("authority-wrong-side", &runners).await;
    runners.side_down(Side::Untrusted);
    runners.create(Side::Trusted, 1, "pull_request", "refs/pull/1/merge", SHA);
    let reply = post(
        &authority,
        entry(
            "1-pull_request",
            "untrusted",
            "pull_request",
            "refs/pull/1/merge",
        ),
    )
    .await;
    assert_eq!(
        reply, 404,
        "an entry for a side that is not up is not recorded"
    );
    let runners = FakeRunners::default();
    let authority = self::authority("authority-wrong-side-up", &runners).await;
    assert_eq!(
        post(
            &authority,
            entry(
                "1-pull_request",
                "untrusted",
                "pull_request",
                "refs/pull/1/merge"
            )
        )
        .await,
        200
    );
    runners.create(Side::Trusted, 1, "pull_request", "refs/pull/1/merge", SHA);
    runners.create(Side::Untrusted, 1, "pull_request", "refs/pull/1/merge", SHA);
    let (_, trusted) = run_facts(&authority, "trusted", 1).await;
    assert_eq!(trusted["refusal"], "wrong_side", "{trusted}");
    let (_, untrusted) = run_facts(&authority, "untrusted", 1).await;
    assert_eq!(untrusted["ledger"], "1-pull_request", "{untrusted}");
}

#[tokio::test]
async fn authority_refuses_a_run_created_before_its_ledger_entry() {
    let runners = FakeRunners::default();
    let authority = authority("authority-created-first", &runners).await;
    runners.create(Side::Trusted, 1, "push", "refs/heads/main", SHA);
    assert_eq!(
        post(
            &authority,
            entry("1-push", "trusted", "push", "refs/heads/main")
        )
        .await,
        200
    );
    let (_, early) = run_facts(&authority, "trusted", 1).await;
    assert_eq!(early["ledger"], Value::Null, "{early}");
    assert_eq!(early["refusal"], "not_dispatched", "{early}");
    runners.create(Side::Trusted, 2, "push", "refs/heads/main", SHA);
    let (_, late) = run_facts(&authority, "trusted", 2).await;
    assert_eq!(late["ledger"], "1-push", "{late}");
}

#[tokio::test]
async fn authority_keeps_the_ledger_on_disk_and_rejects_forged_or_repeated_entries() {
    let runners = FakeRunners::default();
    let dir = temp_dir("authority-ledger");
    let at = dir.join("authority");
    let first = Authority::open(&runners, TOKEN.to_owned(), &at)
        .await
        .unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(
        post(
            &first,
            entry("1-push", "trusted", "push", "refs/heads/main")
        )
        .await,
        200
    );
    assert_eq!(
        post(
            &first,
            entry("1-push", "trusted", "push", "refs/heads/main")
        )
        .await,
        412
    );
    let mut forged = entry("2-push", "trusted", "push", "refs/heads/main");
    forged["existing"] = json!([]);
    assert_eq!(post(&first, forged).await, 400);
    let mut empty = entry("3-push", "trusted", "push", "refs/heads/main");
    empty["sha"] = json!("");
    assert_eq!(post(&first, empty).await, 400);
    let text = std::fs::read_to_string(at.join("ledger.jsonl")).unwrap_or_default();
    assert_eq!(text.lines().count(), 1, "{text}");
    runners.create(Side::Trusted, 1, "push", "refs/heads/main", SHA);
    let reopened = Authority::open(&runners, TOKEN.to_owned(), &at)
        .await
        .unwrap_or_else(|e| unreachable!("{e}"));
    let (_, run) = run_facts(&reopened, "trusted", 1).await;
    assert_eq!(run["ledger"], "1-push", "{run}");
}

#[tokio::test]
async fn authority_mints_a_jit_for_a_side_that_is_up_and_deletes_agents_by_name() {
    let runners = FakeRunners::default();
    let authority = authority("authority-agents", &runners).await;
    let (status, jit) = ask(
        &authority,
        "POST",
        "/jit",
        Some(TOKEN),
        json!({"side": "trusted"}),
    )
    .await;
    assert_eq!((status, jit["jit"].as_str()), (200, Some("e30=")), "{jit}");
    let (status, _) = ask(
        &authority,
        "POST",
        "/jit",
        Some(TOKEN),
        json!({"side": "bogus"}),
    )
    .await;
    assert_eq!(status, 400);
    runners
        .agents
        .lock()
        .unwrap_or_else(|e| unreachable!("{e}"))
        .push((Side::Trusted, 7, "pair-1".to_owned()));
    let delete = |name: &'static str, side: &'static str| {
        ask(
            &authority,
            "DELETE",
            Box::leak(format!("/agents/{side}/{name}").into_boxed_str()),
            Some(TOKEN),
            Value::Null,
        )
    };
    assert_eq!(
        delete("pair-1", "trusted").await,
        (200, json!({"name": "pair-1", "deleted": true}))
    );
    assert_eq!(
        delete("pair-1", "trusted").await,
        (200, json!({"name": "pair-1", "deleted": false}))
    );
    runners.side_down(Side::Untrusted);
    assert_eq!(delete("pair-1", "untrusted").await.0, 404);
}

#[tokio::test]
async fn authority_does_not_report_an_agent_as_deleted_while_runner_server_still_lists_it() {
    let runners = FakeRunners {
        stubborn: true,
        ..FakeRunners::default()
    };
    let authority = authority("authority-stubborn", &runners).await;
    runners
        .agents
        .lock()
        .unwrap_or_else(|e| unreachable!("{e}"))
        .push((Side::Trusted, 7, "pair-1".to_owned()));
    let (status, _) = ask(
        &authority,
        "DELETE",
        "/agents/trusted/pair-1",
        Some(TOKEN),
        Value::Null,
    )
    .await;
    assert_eq!(status, 502);
}

#[test]
fn authority_routes_only_its_four_calls() {
    assert_eq!(call("POST", "/ledger"), Call::Ledger);
    assert_eq!(call("POST", "/jit"), Call::Jit);
    assert_eq!(
        call("GET", "/runs/trusted/12/2"),
        Call::Run {
            side: Side::Trusted,
            id: 12,
            attempt: 2
        }
    );
    assert_eq!(
        call("DELETE", "/agents/untrusted/pair-1"),
        Call::Agent {
            side: Side::Untrusted,
            name: "pair-1"
        }
    );
    for (method, path) in [
        ("GET", "/ledger"),
        ("GET", "/runs/other/1/1"),
        ("GET", "/runs/trusted/x/1"),
        ("GET", "/runs/trusted/1"),
        ("DELETE", "/agents/trusted/a%2Fb"),
        ("DELETE", "/agents/trusted/"),
        ("PUT", "/jit"),
    ] {
        assert_eq!(call(method, path), Call::Unknown, "{method} {path}");
    }
}

#[test]
fn authority_admits_by_event_ref_sha_and_workflow_before_the_run() {
    let facts = |id: u64, workflow: &str| Facts {
        id,
        attempt: 1,
        event: "push".to_owned(),
        git_ref: "refs/heads/main".to_owned(),
        sha: SHA.to_owned(),
        workflow: workflow.to_owned(),
        jobs: vec!["ci".to_owned()],
    };
    let entry = |side, workflow: Option<&str>, existing: Vec<u64>| Entry {
        id: "1-push".to_owned(),
        side,
        event: "push".to_owned(),
        git_ref: "refs/heads/main".to_owned(),
        sha: SHA.to_owned(),
        workflow: workflow.map(str::to_owned),
        existing,
    };
    let entries = [entry(Side::Trusted, Some("a.yml"), vec![])];
    assert!(admit(&entries, Side::Trusted, &facts(1, "a.yml")).is_ok());
    assert_eq!(
        admit(&entries, Side::Trusted, &facts(1, "b.yml")),
        Err(Refusal::NotDispatched)
    );
    assert_eq!(
        admit(&entries, Side::Untrusted, &facts(1, "a.yml")),
        Err(Refusal::WrongSide)
    );
    let seen = [entry(Side::Trusted, None, vec![1])];
    assert_eq!(
        admit(&seen, Side::Trusted, &facts(1, "a.yml")),
        Err(Refusal::NotDispatched)
    );
    assert!(admit(&seen, Side::Trusted, &facts(2, "a.yml")).is_ok());
}

#[test]
fn authority_has_no_address_option_and_run_takes_keep() {
    use clap::{CommandFactory, Parser};
    use velnor_local::cli::{Cli, Command};
    let supervise = Cli::command()
        .find_subcommand("supervise")
        .map(|command| (command.is_hide_set(), command.get_arguments().count()));
    assert_eq!(supervise, Some((true, 1)));
    let parsed = Cli::try_parse_from([
        "velnor-local",
        "run",
        "warm",
        "--dir",
        "d",
        "--repo",
        "r",
        "--keep",
    ]);
    assert!(matches!(parsed, Ok(Cli { command: Command::Run(args) }) if args.keep));
    let kept = Cli::try_parse_from(["velnor-local", "run", "warm", "--dir", "d", "--repo", "r"]);
    assert!(matches!(kept, Ok(Cli { command: Command::Run(args) }) if !args.keep));
    assert!(
        Cli::try_parse_from([
            "velnor-local",
            "supervise",
            "--dir",
            "d",
            "--listen",
            "0.0.0.0:1"
        ])
        .is_err()
    );
}
