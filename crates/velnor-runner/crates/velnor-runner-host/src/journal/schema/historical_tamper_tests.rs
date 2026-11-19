// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::journal::Journal;

type TestResult<T> = Result<T, Box<dyn std::error::Error>>;

/// Versions whose journals a dropped trigger must keep from opening.
///
/// Versions 16 to 19 and 23 to 25 are excluded because origin/main accepts the
/// same tamper there too (measured by running this test over 16..=51 on main:
/// accepted at 16, 17, 18, 19, 23, 24, 25). A later migration reinstalls the
/// dropped trigger before any validator runs: `from_version_sixteen` and
/// `from_version_eighteen` call `provisioning::upgrade_transition_guards` and
/// the revision-trigger installers, `from_version_nineteen` replaces the
/// cleanup guards and installs the v20 revision triggers, and
/// `from_version_twenty_six` reruns `upgrade_transition_guards` and the
/// storage-root, publication and authorization upgrades. So the dropped
/// trigger is repaired, not accepted as tampered, on both main and this branch.
fn tampered_versions() -> impl Iterator<Item = i64> {
    (20..=22).chain(26..=51)
}

#[tokio::test]
async fn a_tampered_journal_at_any_historical_version_is_rejected_on_open() -> TestResult<()> {
    let directory = tempfile::tempdir()?;
    let live_directory = directory.path().join("live");
    std::fs::create_dir(&live_directory)?;
    let live = live_directory.join("journal.db");
    let mut accepted = Vec::new();
    for version in tampered_versions() {
        drop(
            Journal::open_unvalidated_historical_for_test(&live, version)
                .await
                .map_err(|error| format!("advance live to {version}: {error:?}"))?,
        );
        let tampered_directory = directory.path().join(format!("tampered-{version}"));
        std::fs::create_dir(&tampered_directory)?;
        for entry in std::fs::read_dir(&live_directory)? {
            let entry = entry?;
            std::fs::copy(entry.path(), tampered_directory.join(entry.file_name()))?;
        }
        let tampered = tampered_directory.join("journal.db");
        let journal = Journal::open_unvalidated_historical_for_test(&tampered, version)
            .await
            .map_err(|error| format!("reopen copy at {version}: {error:?}"))?;
        let connection = journal.connection().await?;
        let mut rows = connection
            .query(
                "SELECT name FROM sqlite_schema WHERE type='trigger' ORDER BY rowid DESC LIMIT 1",
                (),
            )
            .await?;
        let name: String = rows.next().await?.ok_or("no trigger")?.get(0)?;
        drop(rows);
        connection
            .execute(&format!("DROP TRIGGER {name}"), ())
            .await?;
        drop(connection);
        drop(journal);
        if Journal::open(&tampered).await.is_ok() {
            accepted.push(version);
        }
    }
    assert!(
        accepted.is_empty(),
        "tampered journals opened at {accepted:?}"
    );
    Ok(())
}
