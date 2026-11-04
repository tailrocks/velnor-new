// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use sha1_checked::{Digest, Sha1};
use std::{cell::Cell, collections::BTreeMap, rc::Rc};
use velnor_actions_orchestrator::{
    OrchestratorError,
    crate_release_journal::{JournalIo, JournalTag},
    crate_release_journal_adapter::{JournalAdapter, JournalTransport, RemoteTag},
};

struct Transport(RemoteTag, Rc<Cell<usize>>, Rc<BTreeMap<String, Vec<u8>>>);
impl JournalTransport for Transport {
    fn check_repository_access(&self, _: &str) -> Result<(), OrchestratorError> {
        Ok(())
    }
    fn check_tag_rules(&self, _: &str) -> Result<(), OrchestratorError> {
        Ok(())
    }
    fn get_ref(&self, _: &str, _: &str) -> Result<Option<String>, OrchestratorError> {
        self.1.set(self.1.get() + 1);
        Ok(Some(
            if self.1.get().is_multiple_of(2) {
                "a"
            } else {
                "b"
            }
            .repeat(40),
        ))
    }
    fn create_blob(&self, _: &str, _: &[u8]) -> Result<String, OrchestratorError> {
        Ok("a".repeat(40))
    }
    fn create_tag(&self, _: &str, _: &str, _: &str) -> Result<String, OrchestratorError> {
        Ok("b".repeat(40))
    }
    fn create_ref(&self, _: &str, _: &str, _: &str) -> Result<(), OrchestratorError> {
        Ok(())
    }
    fn get_tag(&self, _: &str, _: &str) -> Result<RemoteTag, OrchestratorError> {
        self.1.set(self.1.get() + 1);
        Ok(self.0.clone())
    }
    fn get_blob(&self, _: &str, sha: &str) -> Result<Vec<u8>, OrchestratorError> {
        self.1.set(self.1.get() + 1);
        Ok(self
            .2
            .get(sha)
            .cloned()
            .unwrap_or_else(|| b"receipt".to_vec()))
    }
}

#[test]
fn reconstructs_only_canonical_unsigned_journal_tag() {
    let tag = JournalTag {
        name: "velnor-receipt/42/1/publication-started".to_owned(),
        object_type: "blob".to_owned(),
        object_sha: "a".repeat(40),
        message: "velnor-release-receipt-v1".to_owned(),
    };
    let raw = format!(
        "object {}\ntype blob\ntag velnor-receipt/42/1/publication-started\ntagger Velnor Release <release@velnor.invalid> 0 +0000\n\nvelnor-release-receipt-v1",
        "a".repeat(40)
    );
    let mut digest = Sha1::new();
    digest.update(format!("tag {}\0", raw.len()));
    digest.update(raw.as_bytes());
    let sha = digest
        .try_finalize()
        .hash()
        .iter()
        .fold(String::new(), |mut result, byte| {
            result.push(char::from(b"0123456789abcdef"[usize::from(byte >> 4)]));
            result.push(char::from(b"0123456789abcdef"[usize::from(byte & 15)]));
            result
        });
    let remote = RemoteTag {
        sha,
        tag: tag.clone(),
        tagger_name: "Velnor Release".to_owned(),
        tagger_email: "release@velnor.invalid".to_owned(),
        tagger_date: "1970-01-01T00:00:00Z".to_owned(),
        unsigned: true,
    };
    let reads = Rc::new(Cell::new(0));
    let client = JournalAdapter::new(Transport(remote.clone(), Rc::clone(&reads), Rc::default()));
    assert_eq!(
        client
            .get_tag_object("example/demo-crate", &remote.sha)
            .expect("canonical tag"),
        tag
    );
    client
        .create_ref(
            "example/demo-crate",
            "refs/tags/velnor-receipt/42/1/publication-started",
            &remote.sha,
        )
        .expect("matching ref");
    assert!(
        client
            .create_ref(
                "example/demo-crate",
                "refs/tags/velnor-receipt/42/1/reconciled",
                &remote.sha,
            )
            .is_err()
    );
    assert_eq!(client.get_verified_tag_bytes("example/demo-crate", &remote.sha).expect("tag bytes"), format!("object {}\ntype blob\ntag velnor-receipt/42/1/publication-started\ntagger Velnor Release <release@velnor.invalid> 0 +0000\n\nvelnor-release-receipt-v1", "a".repeat(40)).as_bytes());
    assert_eq!(
        reads.get(),
        1,
        "verified content-addressed tag should be fetched once"
    );
    client
        .get_tag_object("another/demo-crate", &remote.sha)
        .expect("other repository");
    assert_eq!(
        reads.get(),
        2,
        "repository identity must scope cached objects"
    );
    exercise_cached_blobs_and_refs(&client, &reads).expect("cached blobs and refs");
    reject_bad_tag_objects(&remote);
    rejects_invalid_references(&client);
}

fn exercise_cached_blobs_and_refs(
    client: &JournalAdapter<Transport>,
    reads: &Cell<usize>,
) -> Result<(), OrchestratorError> {
    let blob = "8983ecee2f74474eea1756d4f73a6bdec08e25a5";
    assert_eq!(client.get_blob("example/demo-crate", blob)?, b"receipt");
    assert_eq!(client.get_blob("example/demo-crate", blob)?, b"receipt");
    assert_eq!(reads.get(), 3, "verified blob should be fetched once");
    for _ in 0..2 {
        assert!(
            client
                .get_blob("example/demo-crate", &"b".repeat(40))
                .is_err()
        );
    }
    assert_eq!(
        reads.get(),
        5,
        "hash-mismatched responses must not be cached"
    );
    let reference = "refs/tags/velnor-receipt/42/1/publication-started";
    let first = client.get_ref("example/demo-crate", reference)?;
    let second = client.get_ref("example/demo-crate", reference)?;
    assert_ne!(first, second, "mutable refs must be fetched again");
    Ok(())
}

fn reject_bad_tag_objects(remote: &RemoteTag) {
    for changed in [
        RemoteTag {
            tagger_date: "1970-01-01T00:00:01Z".to_owned(),
            ..remote.clone()
        },
        RemoteTag {
            unsigned: false,
            ..remote.clone()
        },
        RemoteTag {
            sha: "c".repeat(40),
            ..remote.clone()
        },
    ] {
        assert!(
            JournalAdapter::new(Transport(changed, Rc::new(Cell::new(0)), Rc::default()))
                .get_verified_tag_bytes("example/demo-crate", &remote.sha)
                .is_err()
        );
    }
}

fn rejects_invalid_references(client: &JournalAdapter<Transport>) {
    for reference in [
        "refs/tags/velnor-receipt/42/1/operation-1",
        "refs/tags/velnor-receipt/42/1/operation-1000",
    ] {
        assert!(client.get_ref("example/demo-crate", reference).is_ok());
    }
    for reference in [
        "refs/tags/velnor-receipt/042/1/publication-started",
        "refs/tags/velnor-receipt/42/01/reconciled",
        "refs/tags/velnor-receipt/42/1/other",
        "refs/tags/velnor-receipt/42/1/operation-0",
        "refs/tags/velnor-receipt/42/1/operation-01",
        "refs/tags/velnor-receipt/42/1/operation-1001",
        "refs/tags/velnor-receipt/42/1/operation--1",
        "refs/tags/velnor-receipt/42/1/publication-started/extra",
    ] {
        assert!(client.get_ref("example/demo-crate", reference).is_err());
    }
}

#[test]
fn verified_blob_cache_remains_bounded() {
    let mut objects = BTreeMap::new();
    let mut keys = Vec::new();
    for index in 0..129 {
        let value = format!("fixture-{index}").into_bytes();
        let mut digest = Sha1::new();
        digest.update(format!("blob {}\0", value.len()));
        digest.update(&value);
        let sha = digest
            .try_finalize()
            .hash()
            .iter()
            .fold(String::new(), |mut text, byte| {
                text.push(char::from(b"0123456789abcdef"[usize::from(byte >> 4)]));
                text.push(char::from(b"0123456789abcdef"[usize::from(byte & 15)]));
                text
            });
        keys.push(sha.clone());
        objects.insert(sha, value);
    }
    let remote = RemoteTag {
        sha: String::new(),
        tag: JournalTag {
            name: String::new(),
            object_type: String::new(),
            object_sha: String::new(),
            message: String::new(),
        },
        tagger_name: String::new(),
        tagger_email: String::new(),
        tagger_date: String::new(),
        unsigned: true,
    };
    let reads = Rc::new(Cell::new(0));
    let client = JournalAdapter::new(Transport(remote, Rc::clone(&reads), Rc::new(objects)));
    for sha in &keys {
        client
            .get_blob("example/demo-crate", sha)
            .expect("verified blob");
    }
    client
        .get_blob("example/demo-crate", &keys[0])
        .expect("cached first blob");
    assert_eq!(reads.get(), 129);
    client
        .get_blob("example/demo-crate", &keys[128])
        .expect("uncached last blob");
    assert_eq!(reads.get(), 130);
}
