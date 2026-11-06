// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::*;

#[test]
fn journal_owner_identity_includes_kind_and_preserves_canonical_order()
-> Result<(), Box<dyn std::error::Error>> {
    let user = JournalOwner {
        id: 42,
        login: "alice".into(),
        kind: JournalOwnerKind::User,
    };
    let team = JournalOwner {
        id: 42,
        login: "github:org:team".into(),
        kind: JournalOwnerKind::Team,
    };
    let inspect = |owners: Vec<JournalOwner>| -> Result<_, Box<dyn std::error::Error>> {
        let journal = Registry::default();
        let mut record = receipt("0.1.1");
        record.packages[0].owners =
            crate::crate_release_journal::JournalOwnership::Existing { owners };
        seed_started_record(&journal, &record.identity, &record)?;
        Ok(inspect_journal(
            &journal,
            &record.identity,
            JournalStage::Started,
        ))
    };
    assert!(inspect(vec![user.clone(), team.clone()])?.is_ok());
    assert!(inspect(vec![team.clone(), user.clone()])?.is_err());
    let duplicate_user = JournalOwner {
        login: "another".into(),
        ..user.clone()
    };
    assert!(inspect(vec![user, duplicate_user, team])?.is_err());
    Ok(())
}
