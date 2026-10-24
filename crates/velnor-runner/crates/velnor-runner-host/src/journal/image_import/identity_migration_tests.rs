//! Historical image-import migration regressions.

use crate::error::HostError;
use crate::journal::Journal;

use super::{Scratch, request};

#[path = "identity_migration_tests/v6_fixture.rs"]
mod v6_fixture;

#[tokio::test]
async fn v6_rows_migrate_with_nullable_extension_and_keep_history() -> Result<(), HostError> {
    let scratch = Scratch::new()?;
    let path = scratch.file();
    v6_fixture::seed(&path).await?;
    let journal = Journal::open(&path).await?;
    assert_eq!(
        crate::journal_schema::engine_id(&journal.connection().await?).await?,
        "engine-a"
    );
    assert_migrated_image_import_history(&journal).await
}

async fn assert_migrated_image_import_history(journal: &Journal) -> Result<(), HostError> {
    let mut rows = journal
        .connection()
        .await?
        .query(
            "SELECT state, image_id, image_index_digest, image_index_size, manifest_size, load_archive_sha256, load_archive_index_digest, image_reference, receipt FROM image_imports",
            (),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let row = rows
        .next()
        .await
        .map_err(|_| HostError::Journal)?
        .ok_or(HostError::Journal)?;
    assert_eq!(
        row.get::<String>(0).map_err(|_| HostError::Journal)?,
        "completed"
    );
    assert_eq!(
        row.get::<Option<String>>(1)
            .map_err(|_| HostError::Journal)?
            .as_deref(),
        Some(format!("sha256:{}", "f".repeat(64)).as_str())
    );
    for column in [2, 5, 6, 7, 8] {
        assert!(
            row.get::<Option<String>>(column)
                .map_err(|_| HostError::Journal)?
                .is_none()
        );
    }
    for column in [3, 4] {
        assert!(
            row.get::<Option<i64>>(column)
                .map_err(|_| HostError::Journal)?
                .is_none()
        );
    }
    assert_eq!(
        journal.prepare_image_import(request("engine-a")?).await,
        Err(HostError::Identity)
    );
    Ok(())
}
