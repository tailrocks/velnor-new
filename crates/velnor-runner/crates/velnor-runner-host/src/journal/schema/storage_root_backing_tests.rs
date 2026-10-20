use crate::error::HostError;

#[tokio::test]
async fn v27_path_repair_preserves_rows_and_accepts_the_current_home_layout()
-> Result<(), HostError> {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let serial = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let path = std::env::temp_dir().join(format!(
        "velnor-root-path-v27-{}-{serial}.db",
        std::process::id()
    ));
    let database = turso::Builder::new_local(path.to_str().ok_or(HostError::Journal)?)
        .build()
        .await
        .map_err(|_| HostError::Journal)?;
    let connection = database.connect().map_err(|_| HostError::Journal)?;
    setup_legacy_rows(&connection).await?;
    super::upgrade_v27(&connection).await?;
    assert_current_schema(&connection, false).await?;
    assert_legacy_rows_preserved(&connection).await?;
    assert_current_paths_and_rejections(&connection).await?;
    Ok(())
}

#[path = "storage_root_backing_tests/current_journal_reopen.rs"]
mod current_journal_reopen;
#[path = "storage_root_backing_tests/v27_compat.rs"]
mod v27_compat;

async fn setup_legacy_rows(connection: &turso::Connection) -> Result<(), HostError> {
    connection
        .execute(
            "CREATE TABLE capacity_reservation_provisioning (reservation_id INTEGER PRIMARY KEY)",
            (),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    connection
        .execute(
            "INSERT INTO capacity_reservation_provisioning VALUES (1), (2)",
            (),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    connection
        .execute(
            "CREATE TABLE capacity_reservation_provisioning_state (reservation_id INTEGER PRIMARY KEY, resources_bound INTEGER NOT NULL)",
            (),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    connection
        .execute(
            "INSERT INTO capacity_reservation_provisioning_state VALUES (1, 0), (2, 0)",
            (),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    connection
        .execute(
            super::super::provisioning::LEGACY_BOUND_STORAGE_ATTACHMENTS,
            (),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    connection
        .execute(
            "INSERT INTO capacity_reservation_bound_storage_attachments VALUES
            (1, 'runner-home', 'runner', 'runner-id', '/home/runner', NULL, 0, 11, 0, 0, 0, 0, 1, NULL, NULL, '0123456789abcdef', 'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa'),
            (1, 'runner-home', 'dind', 'dind-id', '/home/runner', NULL, 0, 12, 0, 0, 0, 0, 1, NULL, NULL, '0123456789abcdef', 'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa'),
            (1, 'engine-metadata', 'runner', 'runner-id', '/opt/velnor/job-facts', NULL, 1, 13, 0, 0, 0, 0, 1, NULL, NULL, '0123456789abcdef', 'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa')",
            (),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    connection
        .execute(super::LEGACY_ATTACHMENT_PATHS, ())
        .await
        .map_err(|_| HostError::Journal)?;
    connection
        .execute(
            "INSERT INTO capacity_reservation_bound_storage_paths VALUES
            (1, 'runner-home', 'runner', '/home/runner', '/home/runner', NULL),
            (1, 'runner-home', 'dind', '/home/runner', '/home/runner', NULL),
            (1, 'engine-metadata', 'runner', '/opt/velnor/job-facts', '/opt/velnor/job-facts', 'job-facts')",
            (),
        )
        .await
        .map_err(|_| HostError::Journal)?;

    Ok(())
}

async fn assert_current_schema(
    connection: &turso::Connection,
    with_inode: bool,
) -> Result<(), HostError> {
    let actual_attachment_sql =
        table_sql(connection, "capacity_reservation_bound_storage_attachments").await?;
    assert_eq!(
        super::normalize(&actual_attachment_sql),
        super::normalize(super::super::provisioning::BOUND_STORAGE_ATTACHMENTS)
    );
    let actual_path_sql = table_sql(connection, "capacity_reservation_bound_storage_paths").await?;
    let expected_path_sql = if with_inode {
        super::attachment_paths_with_inode()
    } else {
        super::ATTACHMENT_PATHS.to_owned()
    };
    assert_eq!(
        super::normalize(&actual_path_sql),
        super::normalize(&expected_path_sql)
    );

    Ok(())
}

async fn assert_legacy_rows_preserved(connection: &turso::Connection) -> Result<(), HostError> {
    assert_eq!(
        count(connection, "capacity_reservation_bound_storage_attachments").await?,
        3
    );
    assert_eq!(
        count(connection, "capacity_reservation_bound_storage_paths").await?,
        3
    );
    let mut rows = connection
        .query(
            "SELECT volume_subpath FROM capacity_reservation_bound_storage_paths WHERE reservation_id = 1 AND role = 'runner-home' AND observer = 'runner' AND mount_target = '/home/runner'",
            (),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    assert!(
        rows.next()
            .await
            .map_err(|_| HostError::Journal)?
            .ok_or(HostError::Journal)?
            .get::<Option<String>>(0)
            .map_err(|_| HostError::Journal)?
            .is_none()
    );
    assert!(connection
        .execute(
            "UPDATE capacity_reservation_bound_storage_attachments SET mount_id = 99 WHERE reservation_id = 1 AND role = 'runner-home' AND observer = 'runner'",
            (),
        )
        .await
        .is_err());
    Ok(())
}

async fn assert_current_paths_and_rejections(
    connection: &turso::Connection,
) -> Result<(), HostError> {
    insert_attachment(
        connection,
        1,
        "engine-metadata",
        "runner",
        "/opt/velnor/source-proof/control",
        1,
    )
    .await?;
    insert_path(
        connection,
        1,
        "engine-metadata",
        "runner",
        "/opt/velnor/source-proof/control",
        "/opt/velnor/source-proof/control",
        Some("source-proof/control"),
    )
    .await?;
    insert_attachment(
        connection,
        1,
        "runner-home",
        "runner",
        "/opt/velnor/source-proof/inbox",
        0,
    )
    .await?;
    insert_path(
        connection,
        1,
        "runner-home",
        "runner",
        "/opt/velnor/source-proof/inbox",
        "/home/runner/source-proof/inbox",
        Some("source-proof/inbox"),
    )
    .await?;
    insert_attachment(
        connection,
        2,
        "runner-home",
        "runner",
        "/opt/velnor/source-proof/inbox",
        0,
    )
    .await?;
    assert!(
        insert_path(
            connection,
            2,
            "runner-home",
            "runner",
            "/opt/velnor/source-proof/inbox",
            "/home/runner/source-proof/inbox",
            None
        )
        .await
        .is_err()
    );
    assert!(
        insert_path(
            connection,
            2,
            "runner-home",
            "runner",
            "/opt/velnor/source-proof/inbox",
            "/home/runner/source-proof/inbox",
            Some("source-proof/in box")
        )
        .await
        .is_err()
    );
    Ok(())
}

#[test]
fn sql_normalization_preserves_quoted_constraint_literals() {
    let changed = super::ATTACHMENT_PATHS.replace("source-proof/inbox", "source-proof/in box");
    assert_ne!(
        super::normalize(super::ATTACHMENT_PATHS),
        super::normalize(&changed)
    );
}

async fn insert_attachment(
    connection: &turso::Connection,
    reservation_id: i64,
    role: &str,
    observer: &str,
    target: &str,
    read_only: i64,
) -> Result<(), HostError> {
    let container = if observer == "probe" {
        None
    } else {
        Some("container-id")
    };
    connection
        .execute(
            "INSERT INTO capacity_reservation_bound_storage_attachments VALUES (?1, ?2, ?3, ?4, ?5, NULL, ?6, 20, 0, 0, 0, 0, 1, NULL, NULL, '0123456789abcdef', 'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa')",
            turso::params![reservation_id, role, observer, container, target, read_only],
        )
        .await
        .map_err(|_| HostError::Journal)?;
    Ok(())
}

async fn insert_path(
    connection: &turso::Connection,
    reservation_id: i64,
    role: &str,
    observer: &str,
    target: &str,
    root: &str,
    subpath: Option<&str>,
) -> Result<(), HostError> {
    connection
        .execute(
            "INSERT INTO capacity_reservation_bound_storage_paths VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            turso::params![reservation_id, role, observer, target, root, subpath],
        )
        .await
        .map_err(|_| HostError::Journal)?;
    Ok(())
}

async fn count(connection: &turso::Connection, table: &str) -> Result<i64, HostError> {
    let mut rows = connection
        .query(&format!("SELECT COUNT(*) FROM {table}"), ())
        .await
        .map_err(|_| HostError::Journal)?;
    rows.next()
        .await
        .map_err(|_| HostError::Journal)?
        .ok_or(HostError::Journal)?
        .get(0)
        .map_err(|_| HostError::Journal)
}

async fn table_sql(connection: &turso::Connection, table: &str) -> Result<String, HostError> {
    let mut rows = connection
        .query(
            "SELECT sql FROM sqlite_schema WHERE type = 'table' AND name = ?1",
            [table],
        )
        .await
        .map_err(|_| HostError::Journal)?;
    rows.next()
        .await
        .map_err(|_| HostError::Journal)?
        .ok_or(HostError::Journal)?
        .get(0)
        .map_err(|_| HostError::Journal)
}
