// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

mod claims;
mod fake;
mod hashing;
mod mints;

use serde_json::{Value, json};
use velnor_local::authority::{Call, call};
use velnor_local::io::authority::Authority;
use velnor_local::side::Side;

use crate::support::temp_dir;
use fake::{FakeRunners, SHA, TOKEN, ask, authority, entry, post, run_facts};

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
async fn authority_refuses_a_run_of_the_same_event_and_ref_but_another_sha_as_not_dispatched() {
    let runners = FakeRunners::default();
    let authority = authority("authority-other-sha", &runners).await;
    let mut dispatched = entry("1-push", "trusted", "push", "refs/heads/main");
    dispatched["sha"] = json!("1a2b3c4");
    assert_eq!(post(&authority, dispatched).await, 200);
    runners.create(Side::Trusted, 1, "push", "refs/heads/main", SHA);
    let (_, run) = run_facts(&authority, "trusted", 1).await;
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
async fn authority_mints_under_the_name_the_caller_chose_and_refuses_an_invalid_one() {
    let runners = FakeRunners::default();
    let authority = authority("authority-named", &runners).await;
    let (status, jit) = ask(
        &authority,
        "POST",
        "/jit",
        Some(TOKEN),
        json!({"side": "trusted", "name": "host-chosen.1"}),
    )
    .await;
    assert_eq!(
        (status, jit["name"].as_str()),
        (200, Some("host-chosen.1")),
        "{jit}"
    );
    let (status, _) = ask(
        &authority,
        "POST",
        "/jit",
        Some(TOKEN),
        json!({"side": "trusted", "name": "bad name/../x"}),
    )
    .await;
    assert_eq!(status, 400);
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
        json!({"side": "trusted", "name": "agent-1"}),
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
fn authority_has_no_address_option_and_run_takes_keep() {
    use clap::{CommandFactory, Parser};
    use velnor_local::cli::{Cli, Command};
    let supervise = Cli::command().find_subcommand("supervise").map(|command| {
        let names = command.get_arguments().map(|arg| arg.get_id().to_string());
        (command.is_hide_set(), names.collect::<Vec<_>>())
    });
    let names = [
        "dir",
        "velnor_host",
        "pairs",
        "fork_pairs",
        "policy",
        "repo",
    ]
    .map(str::to_owned);
    assert_eq!(supervise, Some((true, names.to_vec())));
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
