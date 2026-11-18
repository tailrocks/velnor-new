// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::path::Path;
use std::time::Duration;

use crate::DaemonLock;
use crate::config::{
    ConnectionId, KeychainReference, RepositoryConnection, ScaleSetSelector, SourcePolicy,
    SourcePolicyMode, WorkflowEvent,
};
use crate::journal::Journal;

use super::{ReservationRow, show_reservations};

type TestResult<T> = Result<T, Box<dyn std::error::Error>>;

const HELPER: &str = "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";

const SCOPE: &str = "INSERT INTO connection_runtime_bindings (connection_id, scale_set_id, scale_set_name, set_ownership, engine_id, docker_endpoint, platform) VALUES ('conn-test-1', 7, 'ubuntu-26.04-scale-set', 'created', 'engine-test', 'unix:///docker.sock', 'linux/amd64');
INSERT INTO host_storage_profiles (generation_id, profile_digest) VALUES ('{hex32}', '{hex64}')";

const RESERVATION: &str = "INSERT INTO connection_intents (id, connection_id, kind, subject, state, scale_set_id, docker_engine_id, launch_phase, assignment_key) VALUES ({i}, 'conn-test-1', 'launch', 'req-{i}', 'uncertain', 7, 'engine-test', 'provisioning_held', 'assign-{i}');
INSERT INTO capacity_reservations (id, intent_id, connection_id, assignment_key, engine_id, docker_root_digest, binding_digest, sample_generation_id, cpu_millicores, memory_bytes, state, remote_cleanup_proven) VALUES ({r}, {i}, 'conn-test-1', 'assign-{i}', 'engine-test', '{hex64}', '{hex64}', '{hex32}', 3000, 6442450944, 'uncertain', 0);
INSERT INTO capacity_reservation_provisioning (reservation_id, operation_id, profile_generation_id, profile_digest, sample_generation_id, observed_at_unix_ms, engine_id, docker_root_digest, retained_memory_bytes) VALUES ({r}, {i}, '{hex32}', '{hex64}', '{hex32}', 1700000000000, 'engine-test', '{hex64}', 0);
INSERT INTO capacity_reservation_provisioning_topology (reservation_id, topology_version) VALUES ({r}, 2);
INSERT INTO capacity_reservation_provisioning_state (reservation_id, bootstrap_start_state) VALUES ({r}, 'not_requested')";

const INITIALIZER: &str = "INSERT INTO capacity_reservation_storage_initialization (reservation_id, connection_id, operation_id, generation, engine_id, docker_root_digest, image_runtime_id, image_source_sha, nano_cpus, memory_bytes, pids_limit, home_volume, work_volume, layout_version, metadata_volume, metadata_volume_id, helper_name, create_dispatch_state, create_state, start_state, result_state, remove_state, abort_state, abort_helper_id, operation_key) VALUES ({r}, 'conn-test-1', {i}, 1, 'engine-test', '{hex64}', 'sha256:{hex64}', '{hex40}', 500000000, 67108864, 16, 'w{r}-home', 'w{r}-work', 2, 'w{r}-metadata', 'w{r}-metadata', 'velnor-init-{i}-1', 'outcome_unknown', 'uncertain', 'not_requested', 'not_requested', 'not_requested', 'absent', '{helper}', '{i}-1')";

fn connection() -> TestResult<RepositoryConnection> {
    Ok(RepositoryConnection::try_new(
        ConnectionId::parse("conn-test-1")?,
        42,
        "owner/repo".to_owned(),
        KeychainReference::parse("keychain:velnor.test/conn-test-1")?,
        ScaleSetSelector::try_new(
            "ubuntu-26.04-scale-set",
            &["velnor".to_owned(), "ubuntu-26.04-scale-set".to_owned()],
        )?,
        SourcePolicy::try_new(
            SourcePolicyMode::BranchRestricted,
            vec!["main".to_owned()],
            Vec::new(),
            vec![WorkflowEvent::Push],
            Vec::new(),
        )?,
    )?)
}

fn fill(template: &str, reservation: i64) -> String {
    template
        .replace("{hex32}", &"b".repeat(32))
        .replace("{hex40}", &"1".repeat(40))
        .replace("{hex64}", &"a".repeat(64))
        .replace("{helper}", HELPER)
        .replace("{r}", &reservation.to_string())
        .replace("{i}", &(reservation - 1).to_string())
}

async fn run(journal: &Journal, statements: &str) -> TestResult<()> {
    let connection = journal.connection().await?;
    for statement in statements.split(";\n") {
        connection.execute(statement, ()).await?;
    }
    Ok(())
}

async fn fixture(state: &Path) -> TestResult<Journal> {
    let journal = Journal::open(&state.join("launch.db")).await?;
    journal.bind_engine("engine-test").await?;
    journal.connection_scope(&connection()?).await?;
    run(&journal, &fill(SCOPE, 0)).await?;
    run(&journal, &fill(RESERVATION, 2)).await?;
    for guard in [
        "storage_initialization_insert_guard",
        "storage_initialization_v2_insert_guard",
    ] {
        let drop_guard = format!("DROP TRIGGER {guard}");
        journal
            .connection()
            .await?
            .execute(drop_guard.as_str(), ())
            .await?;
    }
    run(&journal, &fill(INITIALIZER, 2)).await?;
    run(&journal, &fill(RESERVATION, 4)).await?;
    Ok(journal)
}

fn recovered_line() -> String {
    format!(
        "reservation=2 connection=conn-test-1 reservation_state=uncertain intent_state=uncertain create_dispatch_state=outcome_unknown create_state=uncertain start_state=not_requested abort_state=absent helper_id=- abort_helper_id={HELPER}"
    )
}

const BARE_LINE: &str = "reservation=4 connection=conn-test-1 reservation_state=uncertain intent_state=uncertain create_dispatch_state=- create_state=- start_state=- abort_state=- helper_id=- abort_helper_id=-";

fn lines(rows: &[ReservationRow]) -> Vec<String> {
    rows.iter().map(ReservationRow::render).collect()
}

#[tokio::test]
async fn prints_one_exact_line_per_reservation() -> TestResult<()> {
    let state = tempfile::tempdir()?;
    let journal = fixture(state.path()).await?;
    drop(journal);
    let path = state.path().to_owned();
    let rows = tokio::task::spawn_blocking(move || show_reservations(&path, None)).await??;
    assert_eq!(lines(&rows), [recovered_line(), BARE_LINE.to_owned()]);
    let path = state.path().to_owned();
    let one = tokio::task::spawn_blocking(move || show_reservations(&path, Some(2))).await??;
    assert_eq!(lines(&one), [recovered_line()]);
    let path = state.path().to_owned();
    let none = tokio::task::spawn_blocking(move || show_reservations(&path, Some(9))).await??;
    assert_eq!(lines(&none), Vec::<String>::new());
    Ok(())
}

type FileState = (String, Vec<u8>, std::time::SystemTime);

fn snapshot(state: &Path) -> TestResult<Vec<FileState>> {
    let mut files = std::fs::read_dir(state)?
        .map(|entry| {
            let entry = entry?;
            let name = entry.file_name().to_string_lossy().into_owned();
            let modified = entry.metadata()?.modified()?;
            Ok((name, std::fs::read(entry.path())?, modified))
        })
        .collect::<TestResult<Vec<_>>>()?;
    files.sort();
    Ok(files)
}

#[tokio::test]
async fn reading_while_the_daemon_holds_the_journal_neither_blocks_nor_changes_it() -> TestResult<()>
{
    let state = tempfile::tempdir()?;
    let journal = fixture(state.path()).await?;
    let _daemon = DaemonLock::try_acquire_daemon(state.path())?;
    let writer = journal.connection().await?;
    writer.execute("BEGIN IMMEDIATE", ()).await?;
    let before = snapshot(state.path())?;
    let path = state.path().to_owned();
    let shown = tokio::time::timeout(
        Duration::from_secs(20),
        tokio::task::spawn_blocking(move || show_reservations(&path, None)),
    )
    .await???;
    assert_eq!(
        shown.first().map(ReservationRow::render),
        Some(recovered_line())
    );
    assert_eq!(snapshot(state.path())?, before);
    writer.execute("ROLLBACK", ()).await?;
    Ok(())
}

#[test]
fn a_missing_journal_is_an_error_and_is_not_created() -> TestResult<()> {
    let state = tempfile::tempdir()?;
    assert!(show_reservations(state.path(), None).is_err());
    assert!(!state.path().join("launch.db").exists());
    Ok(())
}

#[tokio::test]
async fn reading_leaves_every_journal_file_byte_identical() -> TestResult<()> {
    let state = tempfile::tempdir()?;
    drop(fixture(state.path()).await?);
    let before = snapshot(state.path())?;
    let path = state.path().to_owned();
    tokio::task::spawn_blocking(move || show_reservations(&path, None)).await??;
    assert_eq!(snapshot(state.path())?, before);
    Ok(())
}

#[tokio::test]
async fn reading_a_journal_with_uncheckpointed_wal_frames_changes_nothing() -> TestResult<()> {
    let state = tempfile::tempdir()?;
    let journal = fixture(state.path()).await?;
    journal
        .connection()
        .await?
        .execute("PRAGMA wal_autocheckpoint = 0", ())
        .await?;
    drop(journal);
    let before = snapshot(state.path())?;
    assert!(
        before
            .iter()
            .any(|(name, bytes, _)| name == "launch.db-wal" && !bytes.is_empty())
    );
    let path = state.path().to_owned();
    let shown = tokio::task::spawn_blocking(move || show_reservations(&path, None)).await??;
    assert_eq!(lines(&shown), [recovered_line(), BARE_LINE.to_owned()]);
    assert_eq!(snapshot(state.path())?, before);
    Ok(())
}

#[tokio::test]
async fn reading_a_journal_whose_files_are_read_only_still_succeeds() -> TestResult<()> {
    use std::os::unix::fs::PermissionsExt;

    let state = tempfile::tempdir()?;
    let journal = fixture(state.path()).await?;
    journal
        .connection()
        .await?
        .execute("PRAGMA wal_autocheckpoint = 0", ())
        .await?;
    drop(journal);
    let read_only = std::fs::Permissions::from_mode(0o444);
    for name in ["launch.db", "launch.db-wal"] {
        std::fs::set_permissions(state.path().join(name), read_only.clone())?;
    }
    let path = state.path().to_owned();
    let shown = tokio::task::spawn_blocking(move || show_reservations(&path, None)).await?;
    for name in ["launch.db", "launch.db-wal"] {
        std::fs::set_permissions(
            state.path().join(name),
            std::fs::Permissions::from_mode(0o644),
        )?;
    }
    assert_eq!(lines(&shown?), [recovered_line(), BARE_LINE.to_owned()]);
    Ok(())
}
