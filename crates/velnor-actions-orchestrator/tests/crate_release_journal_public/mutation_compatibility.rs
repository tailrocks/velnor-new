// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::*;

#[test]
fn old_writer_journal_remains_canonical_and_readable() -> Result<(), Box<dyn std::error::Error>> {
    use crate::crate_release_journal::{JournalTag, inspect_journal_mutation};
    let fixture: serde_json::Value =
        serde_json::from_str(include_str!("../fixtures/de8-journal-store.json"))?;
    let remote = Registry::default();
    *remote.refs.borrow_mut() = serde_json::from_value(fixture["refs"].clone())?;
    *remote.blobs.borrow_mut() = serde_json::from_value(fixture["blobs"].clone())?;
    for (id, value) in fixture["tags"].as_object().ok_or("missing tags")? {
        let field = |name| {
            value[name]
                .as_str()
                .ok_or("missing tag field")
                .map(str::to_owned)
        };
        remote.tags.borrow_mut().insert(
            id.clone(),
            JournalTag {
                name: field("name")?,
                object_type: field("object_type")?,
                object_sha: field("object_sha")?,
                message: field("message")?,
            },
        );
    }
    let intended = inspect_journal_mutation(&remote, &identity(), 1)?.ok_or("missing intent")?;
    let observed =
        inspect_journal_mutation(&remote, &identity(), 2)?.ok_or("missing observation")?;
    assert!(intended.archive_sha256.is_none());
    assert!(matches!(
        intended.phase,
        crate::crate_release_journal::JournalMutationPhase::Intended
    ));
    assert!(matches!(
        observed.phase,
        crate::crate_release_journal::JournalMutationPhase::Observed { .. }
    ));
    for bytes in remote.blobs.borrow().values() {
        let value: serde_json::Value = serde_json::from_slice(bytes)?;
        if value.get("mutation").is_some() {
            let record: crate::crate_release_journal::MutationRecord =
                serde_json::from_slice(bytes)?;
            assert_eq!(
                velnor_actions_contract::canonical_json_bytes(&record)?,
                *bytes
            );
        }
    }
    Ok(())
}

#[test]
fn old_pending_upload_cannot_authorize_a_checksum_claim() -> Result<(), Box<dyn std::error::Error>>
{
    use crate::crate_release_controller::verify_current_upload_intent;
    use crate::crate_release_journal::{JournalTag, inspect_journal, inspect_journal_mutation};
    let fixture: serde_json::Value =
        serde_json::from_str(include_str!("../fixtures/de8-pending-store.json"))?;
    let remote = Registry::default();
    *remote.refs.borrow_mut() = serde_json::from_value(fixture["refs"].clone())?;
    *remote.blobs.borrow_mut() = serde_json::from_value(fixture["blobs"].clone())?;
    for (id, value) in fixture["tags"].as_object().ok_or("missing tags")? {
        let field = |name| {
            value[name]
                .as_str()
                .ok_or("missing tag field")
                .map(str::to_owned)
        };
        remote.tags.borrow_mut().insert(
            id.clone(),
            JournalTag {
                name: field("name")?,
                object_type: field("object_type")?,
                object_sha: field("object_sha")?,
                message: field("message")?,
            },
        );
    }
    let started =
        inspect_journal(&remote, &identity(), JournalStage::Started)?.ok_or("missing start")?;
    let pending = inspect_journal_mutation(&remote, &identity(), 1)?.ok_or("missing intent")?;
    assert!(pending.archive_sha256.is_none());
    assert!(matches!(
        pending.phase,
        crate::crate_release_journal::JournalMutationPhase::Intended
    ));
    assert!(inspect_journal_mutation(&remote, &identity(), 2)?.is_none());
    for claim in ["a".repeat(64), "d".repeat(64)] {
        assert!(
            verify_current_upload_intent(
                &remote,
                &identity(),
                &started.receipt,
                1,
                &pending.package,
                &pending.version,
                &claim
            )
            .is_err()
        );
    }
    Ok(())
}
