// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::path::Path;

use super::Journal;
use super::historical::Historical;
use super::test_template::{Seed, publish_into, seed_from};

type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

const VERSIONS: [Historical; 4] = [
    Historical::V45,
    Historical::V46,
    Historical::V47,
    Historical::V48,
];

async fn drop_last_trigger(db: &Path) -> TestResult {
    let database = turso::Builder::new_local(&db.to_string_lossy())
        .build()
        .await?;
    let connection = database.connect()?;
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
    Ok(())
}

async fn open_from(
    root: &Path,
    version: Historical,
    path: &Path,
) -> Result<Journal, crate::error::HostError> {
    Journal::open_historical_seeded(path, version, |target, key| {
        seed_from(root, "stamp", target, key)
    })
    .await
}

#[tokio::test(flavor = "multi_thread")]
async fn a_tampered_template_is_rejected_for_every_historical_version() -> TestResult {
    for version in VERSIONS {
        let directory = tempfile::tempdir()?;
        let source = directory.path().join("source.db");
        drop(Journal::open_historical_seeded(&source, version, |_, _| Seed::Missing).await?);
        let root = directory.path().join("root");
        publish_into(&root, "stamp", &source, version.key())?;
        let intact = directory.path().join("intact.db");
        assert!(
            open_from(&root, version, &intact).await.is_ok(),
            "{version:?}"
        );
        drop_last_trigger(&root.join("stamp").join(version.key()).join("db")).await?;
        let copied = directory.path().join("copied.db");
        assert!(
            open_from(&root, version, &copied).await.is_err(),
            "{version:?} copy of a tampered template was accepted"
        );
    }
    Ok(())
}
