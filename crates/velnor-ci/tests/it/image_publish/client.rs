// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::io::{Cursor, Read};

use velnor_ci::image_candidate::hashing::sha256_hex;
use velnor_ci::image_publish::digest::Digest;
use velnor_ci::image_publish::location::Kind;
use velnor_ci::io::Registry;

use super::registry::{ACTOR, Fake, Flaws, TOKEN};

fn object(data: &[u8]) -> (Digest, String) {
    let text = format!("sha256:{}", sha256_hex(data));
    (
        Digest::parse(&text).unwrap_or_else(|e| unreachable!("{e}")),
        text,
    )
}

fn login(fake: &Fake) -> Registry<'_> {
    Registry::login(fake, ACTOR, TOKEN).unwrap_or_else(|e| unreachable!("{e}"))
}

fn open(bytes: &[u8]) -> impl Fn() -> std::io::Result<Box<dyn Read + Send>> + Sync + use<> {
    let owned = bytes.to_vec();
    move || Ok(Box::new(Cursor::new(owned.clone())))
}

#[test]
fn a_blob_and_a_manifest_are_pushed_by_digest_and_read_back() {
    let fake = Fake::default();
    let registry = login(&fake);
    let (blob, _) = object(b"layer-bytes");
    let (manifest, manifest_text) = object(br#"{"schemaVersion":2}"#);
    registry
        .push_blob(&blob, 11, &open(b"layer-bytes"))
        .unwrap_or_else(|e| unreachable!("{e}"));
    registry
        .push_manifest(&manifest_text, br#"{"schemaVersion":2}"#)
        .unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(registry.exists(Kind::Blobs, &blob, 11).ok(), Some(true));
    assert_eq!(registry.exists(Kind::Blobs, &blob, 12).ok(), None);
    assert!(registry.verify(Kind::Blobs, &blob, 11).is_ok());
    assert!(registry.verify(Kind::Manifests, &manifest, 19).is_ok());
    assert_eq!(fake.blobs(), [blob.as_str().to_owned()]);
}

#[test]
fn pushing_what_the_registry_already_holds_sends_no_upload() {
    let fake = Fake::default();
    let registry = login(&fake);
    let (blob, manifest_text) = (object(b"layer-bytes").0, object(b"{}").1);
    registry
        .push_blob(&blob, 11, &open(b"layer-bytes"))
        .unwrap_or_else(|e| unreachable!("{e}"));
    registry
        .push_manifest(&manifest_text, b"{}")
        .unwrap_or_else(|e| unreachable!("{e}"));
    let before = fake.seen().len();
    assert!(registry.push_blob(&blob, 11, &open(b"layer-bytes")).is_ok());
    assert!(registry.push_manifest(&manifest_text, b"{}").is_ok());
    let after = fake.seen();
    let writes = after[before..]
        .iter()
        .filter(|seen| matches!(seen.method.as_str(), "POST" | "PUT"))
        .count();
    assert_eq!(writes, 0);
}

#[test]
fn a_tag_reference_is_refused_and_never_requested() {
    let fake = Fake::default();
    let registry = login(&fake);
    let requests = fake.seen().len();
    for tag in ["latest", "main", "v1.0", "sha256:abc"] {
        assert!(registry.push_manifest(tag, b"{}").is_err(), "{tag}");
    }
    let other = format!("sha256:{}", "a".repeat(64));
    assert!(registry.push_manifest(&other, b"{}").is_err());
    assert_eq!(fake.seen().len(), requests, "nothing was requested");
    assert_eq!(fake.manifests(), Vec::<String>::new());
}

#[test]
fn credentials_are_never_sent_to_a_token_endpoint_outside_ghcr() {
    let fake = Fake::with(Flaws {
        challenge: Some(
            r#"Bearer realm="https://evil.example/token",service="ghcr.io""#.to_owned(),
        ),
        ..Flaws::default()
    });
    assert!(Registry::login(&fake, ACTOR, TOKEN).is_err());
    let seen = fake.seen();
    assert_eq!(seen.len(), 1, "only the unauthenticated challenge request");
    assert!(seen.iter().all(|seen| seen.authorization.is_none()));
}
