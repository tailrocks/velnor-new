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
    let status = tokio::time::timeout(std::time::Duration::from_secs(2), async {
        tokio::select! {
            (status, _) = first => status,
            (status, _) = second => status,
        }
    })
    .await
    .unwrap_or_else(|error| unreachable!("concurrent mint did not return: {error}"));
    assert_eq!(status, 409);
}

#[tokio::test]
async fn authority_releases_a_cancelled_mint_name_for_retry() {
    let runners = FakeRunners {
        stalled_mint_calls: 1,
        ..FakeRunners::default()
    };
    let authority = authority("authority-mint-cancelled", &runners).await;
    let body = json!({"side": "trusted", "name": "velnor-x-jit-1"});
    {
        let first = ask(&authority, "POST", "/jit", Some(TOKEN), body.clone());
        tokio::pin!(first);
        tokio::time::timeout(std::time::Duration::from_secs(2), async {
            tokio::select! {
                reply = &mut first => unreachable!("stalled mint returned: {reply:?}"),
                () = runners.mint_entered.notified() => {}
            }
        })
        .await
        .unwrap_or_else(|error| unreachable!("mint did not enter backend: {error}"));
    }
    let (status, reply) = tokio::time::timeout(
        std::time::Duration::from_secs(2),
        ask(&authority, "POST", "/jit", Some(TOKEN), body),
    )
    .await
    .unwrap_or_else(|error| unreachable!("retry did not return: {error}"));
    assert_eq!(status, 200, "{reply}");
    assert_eq!(reply["name"], json!("velnor-x-jit-1"));
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

#[tokio::test]
async fn authority_does_not_report_cleanup_when_the_verification_inventory_is_malformed() {
    let runners = FakeRunners {
        mint_fails: true,
        listing_after_delete: Some(b"{"),
        ..FakeRunners::default()
    };
    let authority = authority("authority-mint-garbled", &runners).await;
    let (status, reply) = ask(
        &authority,
        "POST",
        "/jit",
        Some(TOKEN),
        json!({"side": "trusted", "name": "velnor-x-jit-1"}),
    )
    .await;
    assert_eq!(status, 502);
    assert_ne!(reply["cleaned_up"], json!(true), "{reply}");
}

#[test]
fn a_listed_agent_with_an_unusable_id_makes_the_inventory_malformed() {
    use velnor_local::authority::{MalformedInventory, agent_id};
    for listing in [
        r#"{"value":[{"name":"velnor-x-jit-1"}]}"#,
        r#"{"value":[{"name":"velnor-x-jit-1","id":"9"}]}"#,
        r#"{"value":[{"name":"velnor-x-jit-1","id":-1}]}"#,
    ] {
        assert_eq!(
            agent_id(listing.as_bytes(), "velnor-x-jit-1"),
            Err(MalformedInventory),
            "{listing}"
        );
    }
    assert_eq!(
        agent_id(br#"{"value":[{"name":"other","id":3}]}"#, "velnor-x-jit-1"),
        Ok(None)
    );
    assert_eq!(
        agent_id(
            br#"{"value":[{"name":"velnor-x-jit-1","id":9}]}"#,
            "velnor-x-jit-1"
        ),
        Ok(Some(9))
    );
}
