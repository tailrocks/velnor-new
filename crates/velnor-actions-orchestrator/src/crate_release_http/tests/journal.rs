// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::*;

#[test]
fn reads_exact_workflow_attempt_and_rejects_unsafe_attempt_routes() {
    let (origin, worker) = server(vec![reply("200 OK", r#"{"id":42}"#)]);
    let response = client(origin)
        .get("repos/example/demo-crate/actions/runs/42/attempts/1")
        .expect("attempt response");
    assert_eq!(response["id"], 42);
    assert!(
        worker.join().expect("server")[0]
            .contains("/repos/example/demo-crate/actions/runs/42/attempts/1")
    );
    for route in [
        "repos/example/demo-crate/actions/runs/42/attempts/1/extra",
        "repos/example/demo-crate/actions/runs/42/attempts/1?extra=1",
        "repos/example/demo-crate/actions/runs/42/attempts/nope",
    ] {
        assert!(!super::valid_route(route), "accepted {route}");
    }
}

#[test]
fn operation_listing_uses_exact_prefix_and_rejects_malformed_refs() {
    let good = format!(
        r#"[{{"ref":"refs/tags/velnor-receipt/42/1/operation-1","object":{{"type":"tag","sha":"{}"}}}}]"#,
        "a".repeat(40)
    );
    let (origin, worker) = server(vec![reply("200 OK", &good)]);
    let result = client(origin)
        .list_journal_operations("example/demo-crate", 42, 1)
        .expect("listing");
    assert_eq!(
        result,
        [(
            "refs/tags/velnor-receipt/42/1/operation-1".to_owned(),
            "a".repeat(40)
        )]
    );
    let requests = worker.join().expect("server");
    assert!(requests[0].contains(
        "/repos/example/demo-crate/git/matching-refs/tags/velnor-receipt%2F42%2F1%2Foperation-"
    ));
    let linked = format!(
        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nLink: <https://api.github.com/next>; rel=\"next\"\r\nConnection: close\r\n\r\n{good}",
        good.len()
    );
    for response in [
        linked,
        reply("403 Forbidden", "{}"),
        reply("429 Too Many Requests", "{}"),
        reply("500 Internal Server Error", "{}"),
        reply("302 Found", "{}"),
    ] {
        let (origin, worker) = server(vec![response]);
        assert!(
            client(origin)
                .list_journal_operations("example/demo-crate", 42, 1)
                .is_err()
        );
        worker.join().expect("server");
    }
    for bad in [
        good.replace("operation-1", "operation-01"),
        good.replace("operation-1", "operation-1001"),
        good.replace(&"a".repeat(40), "not-a-sha"),
        good.replace("\"tag\"", "\"commit\""),
        "{}".to_owned(),
        format!(
            "[{},{}]",
            &good[1..good.len() - 1],
            &good[1..good.len() - 1]
        ),
    ] {
        let (origin, worker) = server(vec![reply("200 OK", &bad)]);
        assert!(
            client(origin)
                .list_journal_operations("example/demo-crate", 42, 1)
                .is_err()
        );
        worker.join().expect("server");
    }
}

#[test]
fn operation_names_are_bounded_and_canonical() {
    for value in [
        "velnor-receipt/42/1/operation-1",
        "velnor-receipt/42/1/operation-1000",
    ] {
        assert!(super::validate_journal_name(value).is_ok());
    }
    for value in [
        "velnor-receipt/42/1/operation-0",
        "velnor-receipt/42/1/operation-01",
        "velnor-receipt/42/1/operation-1001",
        "velnor-receipt/42/1/operation--1",
        "velnor-receipt/42/1/operation-1/extra",
    ] {
        assert!(super::validate_journal_name(value).is_err());
    }
}

#[test]
fn journal_adapter_reads_only_the_requested_canonical_tag() {
    use crate::{
        crate_release_github::NativeGitHubAdmissionRead, crate_release_journal::JournalIo as _,
        crate_release_journal_adapter::JournalAdapter,
    };
    use sha1_checked::{Digest as _, Sha1};
    let blob = "b6fc4c620b67d95f953a5c1c1230aaab5db5a1b0";
    let name = "velnor-receipt/12/1/publication-started";
    let raw = format!(
        "object {blob}\ntype blob\ntag {name}\ntagger Velnor Release <release@velnor.invalid> 0 +0000\n\nvelnor-release-receipt-v1"
    );
    let mut digest = Sha1::new();
    digest.update(format!("tag {}\0", raw.len()).as_bytes());
    digest.update(raw.as_bytes());
    let sha = digest
        .try_finalize()
        .hash()
        .iter()
        .fold(String::new(), |mut out, byte| {
            use std::fmt::Write as _;
            write!(&mut out, "{byte:02x}").expect("format");
            out
        });
    let tag = format!(
        r#"{{"sha":"{sha}","tag":"{name}","message":"velnor-release-receipt-v1","object":{{"sha":"{blob}","type":"blob"}},"tagger":{{"name":"Velnor Release","email":"release@velnor.invalid","date":"1970-01-01T00:00:00Z"}},"verification":{{"verified":false,"reason":"unsigned","signature":null,"payload":null}}}}"#
    );
    let (origin, worker) = server(vec![reply("200 OK", &tag)]);
    let adapter = JournalAdapter::new(NativeGitHubAdmissionRead {
        reader: client(origin),
    });
    assert_eq!(
        adapter
            .get_verified_tag_bytes("example/demo-crate", &sha)
            .expect("canonical tag"),
        raw.as_bytes()
    );
    let requests = worker.join().expect("server");
    assert!(requests[0].contains(&format!("/repos/example/demo-crate/git/tags/{sha}")));
}

#[test]
fn journal_adapter_rejects_inconsistent_or_signed_tag_responses() {
    use crate::{
        crate_release_github::NativeGitHubAdmissionRead, crate_release_journal::JournalIo as _,
        crate_release_journal_adapter::JournalAdapter,
    };
    let sha = "a1dbc04588c3239fd6552dda558c5c5431a891c5";
    let valid = format!(
        r#"{{"sha":"{sha}","tag":"velnor-receipt/12/1/publication-started","message":"velnor-release-receipt-v1","object":{{"sha":"b6fc4c620b67d95f953a5c1c1230aaab5db5a1b0","type":"blob"}},"tagger":{{"name":"Velnor Release","email":"release@velnor.invalid","date":"1970-01-01T00:00:00Z"}},"verification":{{"verified":false,"reason":"unsigned","signature":null,"payload":null}}}}"#
    );
    for body in [
        valid.replace("\"type\":\"blob\"", "\"type\":\"commit\""),
        valid.replace("\"verified\":false", "\"verified\":true"),
        valid.replace("1970-01-01T00:00:00Z", "1970-01-01T00:00:01Z"),
        "{}".to_owned(),
    ] {
        let (origin, worker) = server(vec![reply("200 OK", &body)]);
        let adapter = JournalAdapter::new(NativeGitHubAdmissionRead {
            reader: client(origin),
        });
        assert!(
            adapter
                .get_verified_tag_bytes("example/demo-crate", sha)
                .is_err()
        );
        assert_eq!(worker.join().expect("server").len(), 1);
    }
}

#[test]
fn journal_transport_binds_tag_ref_and_blob() {
    exercise_journal_transport("velnor-receipt/12/1/publication-started");
    exercise_journal_transport("velnor-receipt/12/1/operation-1");
}

fn exercise_journal_transport(name: &str) {
    use sha1_checked::{Digest as _, Sha1};
    let blob_sha = "b6fc4c620b67d95f953a5c1c1230aaab5db5a1b0";
    let raw = format!(
        "object {blob_sha}\ntype blob\ntag {name}\ntagger Velnor Release <release@velnor.invalid> 0 +0000\n\nvelnor-release-receipt-v1"
    );
    let mut hasher = Sha1::new();
    hasher.update(format!("tag {}\0", raw.len()).as_bytes());
    hasher.update(raw.as_bytes());
    let tag_sha = hasher
        .try_finalize()
        .hash()
        .iter()
        .fold(String::new(), |mut out, byte| {
            use std::fmt::Write as _;
            write!(&mut out, "{byte:02x}").expect("format");
            out
        });
    let reference = format!("refs/tags/{name}");
    let tag = format!(
        r#"{{"sha":"{tag_sha}","tag":"{name}","message":"velnor-release-receipt-v1","object":{{"sha":"{blob_sha}","type":"blob"}},"tagger":{{"name":"Velnor Release","email":"release@velnor.invalid","date":"1970-01-01T00:00:00Z"}},"verification":{{"verified":false,"reason":"unsigned","signature":null,"payload":null}}}}"#
    );
    let reference_body =
        format!(r#"{{"ref":"{reference}","object":{{"sha":"{tag_sha}","type":"tag"}}}}"#);
    let blob =
        format!(r#"{{"sha":"{blob_sha}","encoding":"base64","size":5,"content":"aGVsbG8="}}"#);
    let (origin, worker) = server(vec![
        reply("201 Created", &tag),
        reply("201 Created", &reference_body),
        reply("200 OK", &reference_body),
        reply("200 OK", &tag),
        reply("200 OK", &blob),
    ]);
    let client = client(origin);
    assert_eq!(
        client
            .create_journal_tag("example/demo-crate", name, blob_sha)
            .expect("tag"),
        tag_sha
    );
    client
        .create_journal_ref("example/demo-crate", &reference, &tag_sha)
        .expect("ref");
    assert_eq!(
        client
            .get_journal_ref("example/demo-crate", &reference)
            .expect("read ref"),
        Some(tag_sha.clone())
    );
    assert_eq!(
        client
            .get_journal_tag("example/demo-crate", &tag_sha)
            .expect("read tag")["tag"],
        name
    );
    assert_eq!(
        client
            .get_journal_blob("example/demo-crate", blob_sha)
            .expect("read blob"),
        b"hello"
    );
    assert_eq!(worker.join().expect("server").len(), 5);
}

#[test]
fn journal_blob_read_and_write_reject_wrong_content_and_errors() {
    let sha = "b6fc4c620b67d95f953a5c1c1230aaab5db5a1b0";
    for body in [
        format!(r#"{{"sha":"{sha}","encoding":"base64","size":5,"content":"d29ybGQ="}}"#),
        format!(r#"{{"sha":"{sha}","encoding":"base64","size":6,"content":"aGVsbG8="}}"#),
    ] {
        let (origin, worker) = server(vec![reply("200 OK", &body)]);
        assert!(
            client(origin)
                .get_journal_blob("example/demo-crate", sha)
                .is_err()
        );
        worker.join().expect("server");
    }
    for status in [
        "409 Conflict",
        "422 Unprocessable Entity",
        "500 Internal Server Error",
    ] {
        let (origin, worker) = server(vec![reply(status, "{}")]);
        assert!(
            client(origin)
                .create_journal_blob("example/demo-crate", b"hello")
                .is_err()
        );
        assert_eq!(worker.join().expect("server").len(), 1);
    }
}

#[test]
fn journal_tag_read_rejects_changed_tagger_and_object() {
    let name = "velnor-receipt/12/1/publication-started";
    let blob = "b6fc4c620b67d95f953a5c1c1230aaab5db5a1b0";
    let sha = super::journal_tag_hash(name, blob).expect("sha");
    let valid = serde_json::json!({"sha":sha,"tag":name,"message":"velnor-release-receipt-v1","object":{"sha":blob,"type":"blob"},"tagger":{"name":"Velnor Release","email":"release@velnor.invalid","date":"1970-01-01T00:00:00Z"},"verification":{"verified":false,"reason":"unsigned","signature":null,"payload":null}});
    for field in ["tagger", "object", "verification"] {
        let mut bad = valid.clone();
        bad[field] = serde_json::json!({});
        let (origin, worker) = server(vec![reply("200 OK", &bad.to_string())]);
        assert!(
            client(origin)
                .get_journal_tag("example/demo-crate", &sha)
                .is_err()
        );
        worker.join().expect("server");
    }
}

#[test]
fn journal_blob_write_requires_exact_content_identity() {
    let sha = "b6fc4c620b67d95f953a5c1c1230aaab5db5a1b0";
    let (origin, worker) = server(vec![reply("201 Created", &format!(r#"{{"sha":"{sha}"}}"#))]);
    assert_eq!(
        client(origin)
            .create_journal_blob("example/demo-crate", b"hello")
            .expect("valid blob"),
        sha
    );
    let requests = worker.join().expect("server");
    assert!(requests[0].contains("\"encoding\":\"base64\""));
    assert!(requests[0].contains("\"content\":\"aGVsbG8=\""));
    let (origin, worker) = server(vec![reply(
        "201 Created",
        &format!(r#"{{"sha":"{}"}}"#, "a".repeat(40)),
    )]);
    assert!(
        client(origin)
            .create_journal_blob("example/demo-crate", b"hello")
            .is_err()
    );
    worker.join().expect("server");
}
