// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::path::Path;

use super::Journal;
use super::historical::Historical;
use super::test_template::{Seed, publish_into, seed_from};

type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

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

async fn a_tampered_template_is_rejected(version: Historical) -> TestResult {
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
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn a_tampered_template_is_rejected_for_every_historical_version_v45() -> TestResult {
    a_tampered_template_is_rejected(Historical::V45).await
}

#[tokio::test(flavor = "multi_thread")]
async fn a_tampered_template_is_rejected_for_every_historical_version_v46() -> TestResult {
    a_tampered_template_is_rejected(Historical::V46).await
}

#[tokio::test(flavor = "multi_thread")]
async fn a_tampered_template_is_rejected_for_every_historical_version_v47() -> TestResult {
    a_tampered_template_is_rejected(Historical::V47).await
}

#[tokio::test(flavor = "multi_thread")]
async fn a_tampered_template_is_rejected_for_every_historical_version_v48() -> TestResult {
    a_tampered_template_is_rejected(Historical::V48).await
}
