// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde_json::{Value, json};
use velnor_local::io::authority::Authority;
use velnor_local::side::Side;

use super::fake::{FakeRunners, SHA, TOKEN, ask, authority, entry, post, run_facts};
use crate::support::temp_dir;

#[tokio::test]
async fn authority_binds_an_entry_to_the_run_it_dispatched_and_refuses_a_later_look_alike() {
    let runners = FakeRunners::default();
    let authority = authority("authority-claims", &runners).await;
    assert_eq!(
        post(
            &authority,
            entry("1-push", "trusted", "push", "refs/heads/main")
        )
        .await,
        200
    );
    runners.create(Side::Trusted, 1, "push", "refs/heads/main", SHA);
    let (_, first) = run_facts(&authority, "trusted", 1).await;
    assert_eq!(first["ledger"], "1-push", "{first}");
    runners.create(Side::Trusted, 2, "push", "refs/heads/main", SHA);
    let (_, look_alike) = run_facts(&authority, "trusted", 2).await;
    assert_eq!(look_alike["ledger"], Value::Null, "{look_alike}");
    assert_eq!(look_alike["refusal"], "not_dispatched", "{look_alike}");
    let (_, again) = run_facts(&authority, "trusted", 1).await;
    assert_eq!(again["ledger"], "1-push", "{again}");
}

#[tokio::test]
async fn authority_keeps_a_claim_across_a_restart() {
    let runners = FakeRunners::default();
    let dir = temp_dir("authority-claims-restart");
    let at = dir.join("authority");
    let first = Authority::open(&runners, TOKEN.to_owned(), &at)
        .await
        .unwrap_or_else(|e| unreachable!("{e}"));
    post(
        &first,
        entry("1-push", "trusted", "push", "refs/heads/main"),
    )
    .await;
    runners.create(Side::Trusted, 1, "push", "refs/heads/main", SHA);
    run_facts(&first, "trusted", 1).await;
    runners.create(Side::Trusted, 2, "push", "refs/heads/main", SHA);
    let reopened = Authority::open(&runners, TOKEN.to_owned(), &at)
        .await
        .unwrap_or_else(|e| unreachable!("{e}"));
    let (_, look_alike) = run_facts(&reopened, "trusted", 2).await;
    assert_eq!(look_alike["refusal"], "not_dispatched", "{look_alike}");
    let (_, original) = run_facts(&reopened, "trusted", 1).await;
    assert_eq!(original["ledger"], "1-push", "{original}");
}

#[tokio::test]
async fn authority_admits_one_run_per_workflow_file_for_one_event() {
    let runners = FakeRunners::default();
    let authority = authority("authority-claims-files", &runners).await;
    post(
        &authority,
        entry("1-push", "trusted", "push", "refs/heads/main"),
    )
    .await;
    runners.create_file(Side::Trusted, 1, "ci.yml", "push", "refs/heads/main", SHA);
    runners.create_file(Side::Trusted, 2, "lint.yml", "push", "refs/heads/main", SHA);
    let (_, ci) = run_facts(&authority, "trusted", 1).await;
    let (_, lint) = run_facts(&authority, "trusted", 2).await;
    assert_eq!(ci["ledger"], "1-push", "{ci}");
    assert_eq!(lint["ledger"], "1-push", "{lint}");
}

#[tokio::test]
async fn authority_gives_two_identical_events_one_entry_each() {
    let runners = FakeRunners::default();
    let authority = authority("authority-claims-twice", &runners).await;
    post(
        &authority,
        entry("1-push", "trusted", "push", "refs/heads/main"),
    )
    .await;
    runners.create(Side::Trusted, 1, "push", "refs/heads/main", SHA);
    post(
        &authority,
        entry("2-push", "trusted", "push", "refs/heads/main"),
    )
    .await;
    runners.create(Side::Trusted, 2, "push", "refs/heads/main", SHA);
    let (_, one) = run_facts(&authority, "trusted", 1).await;
    let (_, two) = run_facts(&authority, "trusted", 2).await;
    assert_eq!(
        (one["ledger"].as_str(), two["ledger"].as_str()),
        (Some("1-push"), Some("2-push"))
    );
}

#[tokio::test]
async fn authority_binds_two_identical_events_by_dispatch_order_whatever_the_query_order() {
    let runners = FakeRunners::default();
    let authority = authority("authority-claims-order", &runners).await;
    post(
        &authority,
        entry("1-push", "trusted", "push", "refs/heads/main"),
    )
    .await;
    runners.create(Side::Trusted, 1, "push", "refs/heads/main", SHA);
    post(
        &authority,
        entry("2-push", "trusted", "push", "refs/heads/main"),
    )
    .await;
    runners.create(Side::Trusted, 2, "push", "refs/heads/main", SHA);
    let (_, two) = run_facts(&authority, "trusted", 2).await;
    let (_, one) = run_facts(&authority, "trusted", 1).await;
    assert_eq!(
        (one["ledger"].as_str(), two["ledger"].as_str()),
        (Some("1-push"), Some("2-push"))
    );
}

#[tokio::test]
async fn authority_keeps_no_claim_the_ledger_could_not_persist() {
    let runners = FakeRunners::default();
    let dir = temp_dir("authority-claims-unsaved");
    let at = dir.join("authority");
    let authority = Authority::open(&runners, TOKEN.to_owned(), &at)
        .await
        .unwrap_or_else(|e| unreachable!("{e}"));
    post(
        &authority,
        entry("1-push", "trusted", "push", "refs/heads/main"),
    )
    .await;
    runners.create(Side::Trusted, 1, "push", "refs/heads/main", SHA);
    let ledger = at.join("ledger.jsonl");
    std::fs::remove_file(&ledger).unwrap_or_else(|e| unreachable!("{e}"));
    std::fs::create_dir(&ledger).unwrap_or_else(|e| unreachable!("{e}"));
    let (first, _) = run_facts(&authority, "trusted", 1).await;
    let (retry, body) = run_facts(&authority, "trusted", 1).await;
    assert_eq!((first, retry), (500, 500), "{body}");
}

#[tokio::test]
async fn authority_removes_the_runner_a_failed_jit_registration_left_behind() {
    let runners = FakeRunners {
        mint_fails: true,
        ..FakeRunners::default()
    };
    let authority = authority("authority-mint-cleanup", &runners).await;
    runners
        .agents
        .lock()
        .unwrap_or_else(|e| unreachable!("{e}"))
        .push((Side::Trusted, 9, "velnor-x-jit-1".to_owned()));
    let (status, _) = ask(
        &authority,
        "POST",
        "/jit",
        Some(TOKEN),
        json!({"side": "trusted", "name": "velnor-x-jit-1"}),
    )
    .await;
    let left = runners
        .agents
        .lock()
        .unwrap_or_else(|e| unreachable!("{e}"))
        .len();
    assert_eq!((status, left), (502, 0));
}

#[tokio::test]
async fn authority_records_an_entry_only_when_the_whole_ledger_is_written() {
    use std::os::unix::fs::PermissionsExt;

    let runners = FakeRunners::default();
    let dir = temp_dir("authority-record-unsaved");
    let at = dir.join("authority");
    let authority = Authority::open(&runners, TOKEN.to_owned(), &at)
        .await
        .unwrap_or_else(|e| unreachable!("{e}"));
    let first = entry("1-push", "trusted", "push", "refs/heads/main");
    let second = entry("2-push", "trusted", "push", "refs/heads/main");
    assert_eq!(post(&authority, first).await, 200);
    let set = |mode| std::fs::set_permissions(&at, std::fs::Permissions::from_mode(mode));
    set(0o500).unwrap_or_else(|e| unreachable!("{e}"));
    let refused = post(&authority, second.clone()).await;
    set(0o700).unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(refused, 500);
    assert_eq!(post(&authority, second).await, 200);
    let kept =
        std::fs::read_to_string(at.join("ledger.jsonl")).unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(kept.lines().count(), 2, "{kept}");
}

#[tokio::test]
async fn authority_refuses_a_mint_that_names_no_agent() {
    let runners = FakeRunners::default();
    let authority = authority("authority-nameless", &runners).await;
    for body in [
        json!({"side": "trusted"}),
        json!({"side": "trusted", "name": ""}),
    ] {
        let (status, reply) = ask(&authority, "POST", "/jit", Some(TOKEN), body).await;
        assert_eq!(status, 400, "{reply}");
    }
}
