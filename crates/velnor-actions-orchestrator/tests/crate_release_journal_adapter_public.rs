// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use sha1_checked::{Digest, Sha1};
use velnor_actions_orchestrator::{
    OrchestratorError,
    crate_release_journal::{JournalIo, JournalTag},
    crate_release_journal_adapter::{JournalAdapter, JournalTransport, RemoteTag},
};

struct Transport(RemoteTag);
impl JournalTransport for Transport {
    fn check_repository_access(&self, _: &str) -> Result<(), OrchestratorError> {
        Ok(())
    }
    fn check_tag_rules(&self, _: &str) -> Result<(), OrchestratorError> {
        Ok(())
    }
    fn get_ref(&self, _: &str, _: &str) -> Result<Option<String>, OrchestratorError> {
        Ok(None)
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
        Ok(self.0.clone())
    }
    fn get_blob(&self, _: &str, _: &str) -> Result<Vec<u8>, OrchestratorError> {
        Ok(b"receipt".to_vec())
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
    let client = JournalAdapter(Transport(remote.clone()));
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
            JournalAdapter(Transport(changed))
                .get_verified_tag_bytes("example/demo-crate", &remote.sha)
                .is_err()
        );
    }
    rejects_invalid_references(&client);
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
