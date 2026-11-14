// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::os::unix::fs::PermissionsExt;

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use bytes::Bytes;
use hyper::Uri;
use serde_json::{Value, json};
use velnor_local::io::RESTORES;

use crate::support::{Harness, MAIN, PULL_1, PULL_2, Reply, recorded, token};

const LIMIT: u64 = 10_000_000_000;

async fn replay(harness: &Harness, name: &str, url: Option<&str>, body: Option<Bytes>) -> Reply {
    let call = recorded(name);
    let request = &call["request"];
    let url = url.unwrap_or_else(|| request["url"].as_str().unwrap_or_default());
    let token = token(MAIN);
    let headers: Vec<(&str, String)> = request["headers"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|header| header["name"] != "Content-Length")
        .map(|header| {
            let value = header["value"].as_str().unwrap_or_default();
            (
                header["name"].as_str().unwrap_or_default(),
                value.replace("REDACTED", &token),
            )
        })
        .collect();
    let body = body.unwrap_or_else(|| match &request["body"] {
        Value::Null => Bytes::new(),
        json => Bytes::from(json.to_string()),
    });
    let method = request["method"].as_str().unwrap_or_default();
    harness.send(method, url, &headers, body).await
}

fn archive(len: usize) -> Vec<u8> {
    (0..len)
        .map(|i| u8::try_from(i % 251).unwrap_or_default())
        .collect()
}

#[tokio::test]
async fn recorded_client_calls_save_then_restore_one_archive() {
    let harness = Harness::new("recorded", LIMIT).await;
    let key = recorded("02-create-cache-entry")["request"]["body"]["key"].clone();

    let miss = replay(&harness, "01-get-cache-entry-download-url-miss", None, None).await;
    assert_eq!((miss.status, miss.json()), (200, json!({"ok": false})));

    let created = replay(&harness, "02-create-cache-entry", None, None)
        .await
        .json();
    assert_eq!(created["ok"], true);
    let upload_url = created["signed_upload_url"]
        .as_str()
        .unwrap_or_default()
        .to_owned();
    let signed_upload: Uri = upload_url.parse().unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(signed_upload.host(), Some("github.localhost"));
    assert_eq!(
        signed_upload
            .path()
            .split('/')
            .filter(|part| !part.is_empty())
            .count(),
        3
    );

    let bytes = archive(300_390);
    let uploaded = replay(
        &harness,
        "03-upload-cache",
        Some(&upload_url),
        Some(Bytes::from(bytes.clone())),
    )
    .await;
    assert_eq!(uploaded.status, 201);

    let finalized = replay(&harness, "04-finalize-cache-entry-upload", None, None)
        .await
        .json();
    assert_eq!(finalized["ok"], true);
    assert!(
        finalized["entry_id"]
            .as_str()
            .is_some_and(|id| id.parse::<u64>().is_ok())
    );

    let hit = replay(&harness, "05-get-cache-entry-download-url-hit", None, None)
        .await
        .json();
    assert_eq!((&hit["ok"], &hit["matched_key"]), (&json!(true), &key));
    let download_url = hit["signed_download_url"].as_str().unwrap_or_default();

    let downloaded = replay(&harness, "06-download-cache", Some(download_url), None).await;
    assert_eq!(downloaded.status, 200);
    assert_eq!(
        downloaded
            .headers
            .get("content-length")
            .map(hyper::http::HeaderValue::as_bytes),
        Some(&b"300390"[..])
    );
    assert_eq!(downloaded.body, bytes);
}

#[tokio::test]
async fn a_block_upload_is_committed_in_block_list_order() {
    let harness = Harness::new("blocks", LIMIT).await;
    let created = harness
        .twirp(
            MAIN,
            "CreateCacheEntry",
            &json!({"key": "large", "version": "v"}),
        )
        .await
        .json();
    let url = created["signed_upload_url"].as_str().unwrap_or_default();
    let (first, second) = (
        URL_SAFE_NO_PAD.encode("block-000000+/"),
        "YmxvY2stMDAwMDAx+/A==",
    );
    let encoded = |id: &str| {
        id.replace('+', "%2B")
            .replace('/', "%2F")
            .replace('=', "%3D")
    };

    let put_second = harness
        .put(
            &format!("{url}?comp=block&blockid={}", encoded(second)),
            b"BBBB",
        )
        .await;
    let put_first = harness
        .put(
            &format!("{url}?comp=block&blockid={}", encoded(&first)),
            b"AAAA",
        )
        .await;
    let list = format!(
        r#"<?xml version="1.0" encoding="utf-8"?><BlockList><Latest>{first}</Latest><Latest>{second}</Latest></BlockList>"#
    );
    let committed = harness
        .put(&format!("{url}?comp=blocklist"), list.as_bytes())
        .await;
    assert_eq!(
        (put_second.status, put_first.status, committed.status),
        (201, 201, 201)
    );
    let retried = harness
        .put(&format!("{url}?comp=blocklist"), list.as_bytes())
        .await;
    assert_eq!(retried.status, 201);

    let finalize = json!({"key": "large", "version": "v", "size_bytes": "8"});
    assert_eq!(
        harness
            .twirp(MAIN, "FinalizeCacheEntryUpload", &finalize)
            .await
            .json()["ok"],
        true
    );
    let hit = harness.restore(MAIN, "large").await;
    let downloaded = harness
        .get(hit["signed_download_url"].as_str().unwrap_or_default())
        .await;
    assert_eq!(downloaded.body, Bytes::from_static(b"AAAABBBB"));
}

#[tokio::test]
async fn a_second_save_in_the_same_scope_is_refused_in_json() {
    let harness = Harness::new("refused", LIMIT).await;
    harness.register(1).await;
    assert_eq!(harness.save(MAIN, "tools", b"first").await["ok"], true);

    let again = harness
        .twirp(
            MAIN,
            "CreateCacheEntry",
            &json!({"key": "tools", "version": "v"}),
        )
        .await;
    assert_eq!(again.status, 200);
    assert_eq!(again.json()["ok"], false);
    assert!(
        again.json()["message"]
            .as_str()
            .is_some_and(|m| m.contains("already exists"))
    );

    assert_eq!(
        harness.save(PULL_1, "tools", b"other scope").await["ok"],
        true
    );
}

#[tokio::test]
async fn a_pull_request_reads_the_default_branch_but_not_another_pull_request() {
    let harness = Harness::new("scopes", LIMIT).await;
    let unregistered = harness
        .twirp(
            PULL_2,
            "GetCacheEntryDownloadURL",
            &json!({"key": "main-entry", "version": "v"}),
        )
        .await;
    assert_eq!(unregistered.status, 412);
    assert_eq!(unregistered.json()["code"], "failed_precondition");

    harness.register(1).await;
    harness.register(2).await;
    let rebased = harness
        .send(
            "PUT",
            "http://github.localhost/_velnor/pull-requests/2",
            &[],
            Bytes::from(r#"{"base_branch":"release"}"#),
        )
        .await;
    assert_eq!(
        (rebased.status, rebased.json()["code"].clone()),
        (412, json!("failed_precondition"))
    );
    assert_eq!(harness.save(MAIN, "main-entry", b"main").await["ok"], true);
    assert_eq!(
        harness.save(PULL_1, "pr-entry", b"pull request 1").await["ok"],
        true
    );

    assert_eq!(
        harness.restore(PULL_2, "pr-entry").await,
        json!({"ok": false})
    );
    let hit = harness.restore(PULL_2, "main-entry").await;
    assert_eq!(
        (&hit["ok"], &hit["matched_key"]),
        (&json!(true), &json!("main-entry"))
    );
    assert_eq!(harness.restore(PULL_1, "pr-entry").await["ok"], true);
}

#[tokio::test]
async fn an_expired_token_or_one_without_a_claim_is_refused_in_json() {
    let harness = Harness::new("claims", LIMIT).await;
    for name in ["expired", "missing-default-ref"] {
        let refused = harness
            .twirp(
                name,
                "CreateCacheEntry",
                &json!({"key": "tools", "version": "v"}),
            )
            .await;
        assert_eq!(
            (refused.status, refused.json()["code"].clone()),
            (401, json!("unauthenticated")),
            "{name}"
        );
    }
}

#[tokio::test]
async fn a_token_that_does_not_verify_is_refused_in_json() {
    let harness = Harness::new("token", LIMIT).await;
    let genuine = token(MAIN);
    let mut parts = genuine.split('.');
    let (header, claims, signature) = (parts.next(), parts.next(), parts.next());
    let claims = String::from_utf8(
        URL_SAFE_NO_PAD
            .decode(claims.unwrap_or_default())
            .unwrap_or_default(),
    )
    .unwrap_or_default()
    .replace("refs/heads/main", "refs/heads/release");
    let forged = format!(
        "{}.{}.{}",
        header.unwrap_or_default(),
        URL_SAFE_NO_PAD.encode(claims),
        signature.unwrap_or_default()
    );

    let refused = harness
        .twirp_with(
            &forged,
            "CreateCacheEntry",
            &json!({"key": "tools", "version": "v"}),
        )
        .await;
    assert_eq!(refused.status, 401);
    assert_eq!(refused.json()["code"], "unauthenticated");
}

#[tokio::test]
async fn a_save_over_the_limit_evicts_and_deletes_the_archive() {
    let harness = Harness::new("evict", 1_000).await;
    assert_eq!(
        harness.save(MAIN, "large", &archive(2_000)).await["ok"],
        true
    );
    assert_eq!(harness.save(MAIN, "small", &archive(100)).await["ok"], true);

    assert_eq!(harness.restore(MAIN, "large").await, json!({"ok": false}));
    assert_eq!(harness.restore(MAIN, "small").await["ok"], true);
    let archives = std::fs::read_dir(harness.dir().join("archives")).map_or(0, Iterator::count);
    assert_eq!(archives, 1);
}

#[tokio::test]
async fn a_reservation_that_could_not_be_saved_can_be_retried() {
    let harness = Harness::new("unsaved", LIMIT).await;
    let entry = json!({"key": "other", "version": "v"});
    harness.twirp(MAIN, "CreateCacheEntry", &entry).await;
    let read_only = |mode: u32| {
        std::fs::set_permissions(harness.dir(), std::fs::Permissions::from_mode(mode))
            .unwrap_or_else(|e| unreachable!("{e}"));
    };
    let entry = json!({"key": "tools", "version": "v"});

    read_only(0o555);
    let failed = harness.twirp(MAIN, "CreateCacheEntry", &entry).await;
    read_only(0o755);
    let retried = harness.twirp(MAIN, "CreateCacheEntry", &entry).await;

    assert_eq!(failed.status, 500);
    assert_eq!(retried.json()["ok"], true);
}

#[tokio::test]
async fn the_index_survives_a_restart() {
    let harness = Harness::new("restart", LIMIT).await;
    assert_eq!(
        harness.save(MAIN, "tools", b"kept on disk").await["ok"],
        true
    );

    let harness = harness.reopen().await;
    let hit = harness.restore(MAIN, "tools").await;
    assert_eq!(hit["ok"], true);
    let downloaded = harness
        .get(hit["signed_download_url"].as_str().unwrap_or_default())
        .await;
    assert_eq!(downloaded.body, Bytes::from_static(b"kept on disk"));
}

#[tokio::test]
async fn every_restore_is_journaled_with_its_job_key_entry_and_scope() {
    let harness = Harness::new("journal", LIMIT).await;
    harness.save(MAIN, "cargo-main", b"archive").await;
    harness.register(1).await;
    let fallback = json!({"key": "cargo-mai", "restore_keys": ["cargo-"], "version": "v"});
    let restored = harness
        .twirp(PULL_1, "GetCacheEntryDownloadURL", &fallback)
        .await
        .json();
    assert_eq!(restored["ok"], true);
    assert_eq!(harness.restore(MAIN, "cargo-other").await["ok"], false);
    let journal = std::fs::read_to_string(harness.dir().join(RESTORES))
        .unwrap_or_else(|e| unreachable!("{e}"));
    let lines: Vec<Value> = journal
        .lines()
        .map(|line| serde_json::from_str(line).unwrap_or_else(|e| unreachable!("{e}")))
        .collect();
    assert_eq!(
        lines,
        [
            json!({"writer": "Actions.Results:1:cache", "key": "cargo-mai", "matched_key": "cargo-main", "scope": MAIN}),
            json!({"writer": "Actions.Results:1:cache", "key": "cargo-other", "matched_key": null, "scope": null}),
        ]
    );
}
