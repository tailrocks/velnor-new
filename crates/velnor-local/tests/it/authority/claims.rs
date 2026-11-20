// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde_json::Value;
use velnor_local::authority::{Entry, Facts, Refusal, admit};
use velnor_local::io::authority::Authority;
use velnor_local::side::Side;
use velnor_local::standin::Run;

use super::fake::{FakeRunners, SHA, TOKEN, authority, entry, post, run_facts};
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
async fn authority_admits_every_workflow_one_unscoped_dispatch_created_once_each() {
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
    runners.create_file(Side::Trusted, 3, "lint.yml", "push", "refs/heads/main", SHA);
    let (_, again) = run_facts(&authority, "trusted", 3).await;
    assert_eq!(again["refusal"], "not_dispatched", "{again}");
    runners.create_file(
        Side::Trusted,
        4,
        "ci.yml",
        "pull_request",
        "refs/heads/main",
        SHA,
    );
    let (_, other) = run_facts(&authority, "trusted", 4).await;
    assert_eq!(other["refusal"], "not_dispatched", "{other}");
}

#[tokio::test]
async fn authority_does_not_count_the_other_sides_claim_of_the_same_run_id() {
    let runners = FakeRunners::default();
    let authority = authority("authority-claims-sides", &runners).await;
    post(
        &authority,
        entry("1-push", "untrusted", "push", "refs/heads/main"),
    )
    .await;
    post(
        &authority,
        entry("2-push", "trusted", "push", "refs/heads/main"),
    )
    .await;
    runners.create(Side::Untrusted, 1, "push", "refs/heads/main", SHA);
    runners.create(Side::Trusted, 1, "push", "refs/heads/main", SHA);
    let (_, untrusted) = run_facts(&authority, "untrusted", 1).await;
    let (_, trusted) = run_facts(&authority, "trusted", 1).await;
    assert_eq!(untrusted["ledger"], "1-push", "{untrusted}");
    assert_eq!(trusted["ledger"], "2-push", "{trusted}");
}

#[tokio::test]
async fn authority_gives_the_entry_to_the_lowest_new_run_not_to_a_look_alike_decided_first() {
    let runners = FakeRunners::default();
    let authority = authority("authority-claims-lowest", &runners).await;
    post(
        &authority,
        entry("1-push", "trusted", "push", "refs/heads/main"),
    )
    .await;
    runners.create(Side::Trusted, 1, "push", "refs/heads/main", SHA);
    runners.create(Side::Trusted, 2, "push", "refs/heads/main", SHA);
    let (_, look_alike) = run_facts(&authority, "trusted", 2).await;
    assert_eq!(look_alike["ledger"], Value::Null, "{look_alike}");
    let (_, real) = run_facts(&authority, "trusted", 1).await;
    assert_eq!(real["ledger"], "1-push", "{real}");
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
async fn authority_gives_two_identical_events_their_runs_in_creation_order_when_the_first_is_decided_first()
 {
    let runners = FakeRunners::default();
    let authority = authority("authority-claims-first", &runners).await;
    for id in ["1-push", "2-push"] {
        post(&authority, entry(id, "trusted", "push", "refs/heads/main")).await;
    }
    runners.create(Side::Trusted, 1, "push", "refs/heads/main", SHA);
    runners.create(Side::Trusted, 2, "push", "refs/heads/main", SHA);
    let (_, one) = run_facts(&authority, "trusted", 1).await;
    let (_, two) = run_facts(&authority, "trusted", 2).await;
    assert_eq!(
        (one["ledger"].as_str(), two["ledger"].as_str()),
        (Some("1-push"), Some("2-push"))
    );
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
        claims: Vec::new(),
    };
    let runs = |ids: &[u64], file: &str| -> Vec<Run> {
        ids.iter()
            .map(|id| Run {
                id: *id,
                file_name: file.to_owned(),
                display_name: None,
                event_name: Some("push".to_owned()),
                git_ref: Some("refs/heads/main".to_owned()),
                sha: Some(SHA.to_owned()),
                result: None,
                status: None,
            })
            .collect()
    };
    let entries = [entry(Side::Trusted, Some("a.yml"), vec![])];
    assert!(
        admit(
            &entries,
            Side::Trusted,
            &facts(1, "a.yml"),
            &runs(&[1], "a.yml")
        )
        .is_ok()
    );
    assert_eq!(
        admit(
            &entries,
            Side::Trusted,
            &facts(1, "b.yml"),
            &runs(&[1], "b.yml")
        ),
        Err(Refusal::NotDispatched)
    );
    assert_eq!(
        admit(
            &entries,
            Side::Untrusted,
            &facts(1, "a.yml"),
            &runs(&[1], "a.yml")
        ),
        Err(Refusal::WrongSide)
    );
    let seen = [entry(Side::Trusted, None, vec![1])];
    assert_eq!(
        admit(
            &seen,
            Side::Trusted,
            &facts(1, "a.yml"),
            &runs(&[1], "a.yml")
        ),
        Err(Refusal::NotDispatched)
    );
    assert!(
        admit(
            &seen,
            Side::Trusted,
            &facts(2, "a.yml"),
            &runs(&[1, 2], "a.yml")
        )
        .is_ok()
    );
}
