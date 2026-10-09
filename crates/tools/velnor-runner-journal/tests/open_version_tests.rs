//! Existing-only journal opens must refuse future schemas without migration.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use turso::Builder;
use velnor_runner_journal::{HostError, Journal};

fn scratch(label: &str) -> Result<PathBuf, String> {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let n = NEXT.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "velnor-journal-version-{label}-{}-{n}",
        std::process::id()
    ));
    fs::create_dir_all(&dir).map_err(|error| error.to_string())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&dir, fs::Permissions::from_mode(0o700))
            .map_err(|error| error.to_string())?;
    }
    Ok(dir)
}

fn parent_identity(path: &Path) -> Result<(u64, u64), String> {
    use std::os::unix::fs::MetadataExt;
    let metadata = fs::metadata(path.parent().ok_or("journal path has no parent")?)
        .map_err(|error| error.to_string())?;
    Ok((metadata.dev(), metadata.ino()))
}

fn snapshot_files(dir: &Path) -> Result<BTreeMap<String, Vec<u8>>, String> {
    let mut snapshot = BTreeMap::new();
    for entry in fs::read_dir(dir).map_err(|error| error.to_string())? {
        let entry = entry.map_err(|error| error.to_string())?;
        let kind = entry.file_type().map_err(|error| error.to_string())?;
        if kind.is_file() {
            let name = entry.file_name().to_string_lossy().into_owned();
            let bytes = fs::read(entry.path()).map_err(|error| error.to_string())?;
            snapshot.insert(name, bytes);
        }
    }
    Ok(snapshot)
}

async fn execute(path: &Path, sql: &str) -> Result<(), String> {
    let text = path
        .to_str()
        .ok_or_else(|| "journal path was not UTF-8".to_owned())?;
    let db = Builder::new_local(text)
        .build()
        .await
        .map_err(|error| error.to_string())?;
    let conn = db.connect().map_err(|error| error.to_string())?;
    conn.execute(sql, ())
        .await
        .map_err(|error| error.to_string())
        .map(|_| ())
}

async fn scalar(path: &Path, sql: &str) -> Result<i64, String> {
    let text = path
        .to_str()
        .ok_or_else(|| "journal path was not UTF-8".to_owned())?;
    let db = Builder::new_local(text)
        .build()
        .await
        .map_err(|error| error.to_string())?;
    let conn = db.connect().map_err(|error| error.to_string())?;
    let mut rows = conn
        .query(sql, ())
        .await
        .map_err(|error| error.to_string())?;
    rows.next()
        .await
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "scalar query returned no row".to_owned())?
        .get::<i64>(0)
        .map_err(|error| error.to_string())
}

async fn set_future_v14_shape(path: &Path) -> Result<(), String> {
    execute(
        path,
        "CREATE TABLE v14_future_marker (intent_id INTEGER PRIMARY KEY, marker TEXT NOT NULL)",
    )
    .await?;
    execute(path, "PRAGMA user_version = 14").await
}

fn unsupported_v14(result: &Result<Journal, HostError>) -> bool {
    matches!(
        result,
        Err(HostError::UnsupportedJournalVersion {
            found: 14,
            supported: 13
        })
    )
}

#[tokio::test]
async fn current_v13_existing_open_modes_still_work() -> Result<(), String> {
    let dir = scratch("current")?;
    let path = dir.join("journal.db");
    let journal = Journal::open(&path)
        .await
        .map_err(|error| error.to_string())?;
    journal
        .begin("launch", "current-version-row")
        .await
        .map_err(|error| error.to_string())?;
    drop(journal);

    let readonly = Journal::open_readonly(&path)
        .await
        .map_err(|error| error.to_string())?;
    assert!(
        !readonly
            .draining()
            .await
            .map_err(|error| error.to_string())?
    );
    drop(readonly);

    let (device, inode) = parent_identity(&path)?;
    let protected_readonly = Journal::open_readonly_protected_at(&path, device, inode)
        .await
        .map_err(|error| error.to_string())?;
    assert_eq!(
        protected_readonly
            .rows()
            .await
            .map_err(|e| e.to_string())?
            .len(),
        1
    );
    drop(protected_readonly);

    let writable = Journal::open_existing(&path)
        .await
        .map_err(|error| error.to_string())?;
    assert_eq!(writable.rows().await.map_err(|e| e.to_string())?.len(), 1);
    drop(writable);

    let protected_writable = Journal::open_existing_protected_at(&path, device, inode)
        .await
        .map_err(|error| error.to_string())?;
    protected_writable
        .request_drain()
        .await
        .map_err(|error| error.to_string())?;
    assert!(
        protected_writable
            .draining()
            .await
            .map_err(|e| e.to_string())?
    );
    Ok(())
}

#[tokio::test]
async fn future_v14_opens_refuse_before_existing_only_reads_or_writes() -> Result<(), String> {
    let dir = scratch("future")?;
    let path = dir.join("journal.db");
    let journal = Journal::open(&path)
        .await
        .map_err(|error| error.to_string())?;
    journal
        .begin("launch", "preserved-row")
        .await
        .map_err(|error| error.to_string())?;
    drop(journal);
    set_future_v14_shape(&path).await?;
    let before = snapshot_files(&dir)?;
    let (device, inode) = parent_identity(&path)?;

    assert!(unsupported_v14(&Journal::open_readonly(&path).await));
    assert!(unsupported_v14(&Journal::open_existing(&path).await));
    assert!(unsupported_v14(
        &Journal::open_readonly_protected_at(&path, device, inode).await
    ));
    assert!(unsupported_v14(
        &Journal::open_existing_protected_at(&path, device, inode).await
    ));
    assert_eq!(snapshot_files(&dir)?, before);
    assert!(unsupported_v14(&Journal::open(&path).await));
    assert_eq!(snapshot_files(&dir)?, before);

    assert_eq!(scalar(&path, "PRAGMA user_version").await?, 14);
    assert_eq!(scalar(&path, "SELECT COUNT(*) FROM intents").await?, 1);
    assert_eq!(
        scalar(&path, "SELECT COUNT(*) FROM linux_launch_daemon_bindings").await?,
        0
    );
    assert_eq!(
        scalar(&path, "SELECT draining FROM controller_state WHERE id = 1").await?,
        0
    );
    Ok(())
}

#[tokio::test]
async fn v10_existing_open_remains_available_for_the_existing_migration_path() -> Result<(), String>
{
    let dir = scratch("v10")?;
    let path = dir.join("journal.db");
    let journal = Journal::open(&path)
        .await
        .map_err(|error| error.to_string())?;
    journal
        .begin("launch", "v10-compatible-row")
        .await
        .map_err(|error| error.to_string())?;
    drop(journal);
    execute(&path, "DROP TABLE scale_set_population_observations").await?;
    execute(&path, "DROP TABLE linux_launch_daemon_bindings").await?;
    execute(&path, "DROP TABLE linux_launch_daemon_adoptions").await?;
    execute(&path, "DROP TABLE linux_launch_started_observations").await?;
    execute(&path, "PRAGMA user_version = 10").await?;
    let before = snapshot_files(&dir)?;

    let readonly = Journal::open_readonly(&path)
        .await
        .map_err(|error| error.to_string())?;
    assert_eq!(readonly.rows().await.map_err(|e| e.to_string())?.len(), 1);
    drop(readonly);
    let existing = Journal::open_existing(&path)
        .await
        .map_err(|error| error.to_string())?;
    assert_eq!(existing.rows().await.map_err(|e| e.to_string())?.len(), 1);
    drop(existing);
    assert_eq!(scalar(&path, "PRAGMA user_version").await?, 10);
    assert!(!table_exists(&path, "scale_set_population_observations").await?);
    assert_eq!(snapshot_files(&dir)?, before);

    let migrated = Journal::open(&path)
        .await
        .map_err(|error| error.to_string())?;
    assert_eq!(migrated.rows().await.map_err(|e| e.to_string())?.len(), 1);
    assert_eq!(scalar(&path, "PRAGMA user_version").await?, 13);
    Ok(())
}

#[tokio::test]
async fn v12_existing_only_reads_preserve_the_pre_adoption_schema() -> Result<(), String> {
    let dir = scratch("v12-existing")?;
    let path = dir.join("journal.db");
    let journal = Journal::open(&path)
        .await
        .map_err(|error| error.to_string())?;
    journal
        .begin("launch", "v12-compatible-row")
        .await
        .map_err(|error| error.to_string())?;
    drop(journal);
    execute(&path, "DROP TABLE linux_launch_daemon_adoptions").await?;
    execute(&path, "DROP TABLE linux_launch_started_observations").await?;
    execute(&path, "PRAGMA user_version = 12").await?;
    let before = snapshot_files(&dir)?;
    let (device, inode) = parent_identity(&path)?;

    let readonly = Journal::open_readonly(&path)
        .await
        .map_err(|error| error.to_string())?;
    assert_eq!(readonly.rows().await.map_err(|e| e.to_string())?.len(), 1);
    drop(readonly);
    let protected_readonly = Journal::open_readonly_protected_at(&path, device, inode)
        .await
        .map_err(|error| error.to_string())?;
    assert_eq!(
        protected_readonly
            .rows()
            .await
            .map_err(|e| e.to_string())?
            .len(),
        1
    );
    drop(protected_readonly);
    let existing = Journal::open_existing(&path)
        .await
        .map_err(|error| error.to_string())?;
    assert_eq!(existing.rows().await.map_err(|e| e.to_string())?.len(), 1);
    assert_eq!(
        existing
            .drain_snapshot()
            .await
            .map_err(|e| e.to_string())?
            .occupied_launches,
        1
    );
    drop(existing);
    let protected_existing = Journal::open_existing_protected_at(&path, device, inode)
        .await
        .map_err(|error| error.to_string())?;
    assert_eq!(
        protected_existing
            .rows()
            .await
            .map_err(|e| e.to_string())?
            .len(),
        1
    );
    drop(protected_existing);

    assert_eq!(scalar(&path, "PRAGMA user_version").await?, 12);
    assert!(!table_exists(&path, "linux_launch_started_observations").await?);
    assert!(!table_exists(&path, "linux_launch_daemon_adoptions").await?);
    assert_eq!(snapshot_files(&dir)?, before);
    let migrated = Journal::open(&path)
        .await
        .map_err(|error| error.to_string())?;
    assert_eq!(scalar(&path, "PRAGMA user_version").await?, 13);
    assert!(table_exists(&path, "linux_launch_started_observations").await?);
    assert!(table_exists(&path, "linux_launch_daemon_adoptions").await?);
    assert_eq!(
        scalar(
            &path,
            "SELECT COUNT(*) FROM linux_launch_started_observations"
        )
        .await?,
        0,
        "v12 rows must not be backfilled as observed Started events"
    );
    drop(migrated);
    Ok(())
}

async fn table_exists(path: &Path, table: &str) -> Result<bool, String> {
    Ok(scalar(
        path,
        &format!(
            "SELECT EXISTS (SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = '{table}')"
        ),
    )
    .await?
        == 1)
}

#[tokio::test]
async fn missing_and_negative_version_errors_remain_distinct_from_future_version()
-> Result<(), String> {
    let dir = scratch("invalid")?;
    let missing = dir.join("missing.db");
    assert_eq!(
        Journal::open_existing(&missing).await.err(),
        Some(HostError::Journal)
    );

    let path = dir.join("negative.db");
    let journal = Journal::open(&path)
        .await
        .map_err(|error| error.to_string())?;
    drop(journal);
    execute(&path, "PRAGMA user_version = -1").await?;
    assert_eq!(
        Journal::open_existing(&path).await.err(),
        Some(HostError::Journal)
    );
    assert_eq!(Journal::open(&path).await.err(), Some(HostError::Journal));
    Ok(())
}
