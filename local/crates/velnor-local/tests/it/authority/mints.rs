// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde_json::json;
use velnor_local::io::authority::mint_or_retire;
use velnor_local::side::Side;

use super::fake::{FakeRunners, TOKEN, ask, authority};

#[tokio::test]
async fn authority_removes_the_runner_a_failed_jit_registration_left_behind() {
    let runners = FakeRunners {
        mint_fails: true,
        ..FakeRunners::default()
    };
    let authority = authority("authority-mint-cleanup", &runners).await;
    let (status, reply) = ask(
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
    assert_eq!(reply["cleaned_up"], json!(true), "{reply}");
}

#[tokio::test]
async fn authority_refuses_a_mint_under_a_name_that_is_registered_and_keeps_that_agent() {
    let runners = FakeRunners {
        mint_fails: true,
        ..FakeRunners::default()
    };
    let authority = authority("authority-mint-taken", &runners).await;
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
    let minted = *runners
        .minted
        .lock()
        .unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!((status, left, minted), (409, 1, 0));
}

#[tokio::test]
async fn authority_refuses_a_second_mint_under_a_name_that_is_being_minted() {
    let runners = FakeRunners {
        mint_stalls: true,
        ..FakeRunners::default()
    };
    let authority = authority("authority-mint-inflight", &runners).await;
    let body = json!({"side": "trusted", "name": "velnor-x-jit-1"});
    let first = ask(&authority, "POST", "/jit", Some(TOKEN), body.clone());
    let second = ask(&authority, "POST", "/jit", Some(TOKEN), body);
    let status = tokio::select! {
        (status, _) = first => status,
        (status, _) = second => status,
    };
    assert_eq!(status, 409);
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

#[tokio::test]
async fn a_jit_that_fails_to_pack_leaves_no_agent_registered() {
    let runners = FakeRunners {
        mint_fails: true,
        ..FakeRunners::default()
    };
    runners
        .agents
        .lock()
        .unwrap_or_else(|e| unreachable!("{e}"))
        .push((Side::Trusted, 9, "velnor-x-jit-1".to_owned()));
    let minted = mint_or_retire(&&runners, Side::Trusted, None).await;
    let left = runners
        .agents
        .lock()
        .unwrap_or_else(|e| unreachable!("{e}"))
        .len();
    assert!(minted.is_err());
    assert_eq!(left, 0);
}
