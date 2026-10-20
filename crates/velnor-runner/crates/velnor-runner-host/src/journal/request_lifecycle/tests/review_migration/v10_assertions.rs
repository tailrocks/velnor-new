use std::path::Path;

use crate::error::HostError;

pub(super) async fn assert_cleaned_runner_identity(
    path: &Path,
    runner: &velnor_runner_github::RunnerReference,
) -> Result<(), HostError> {
    let path = path.to_str().ok_or(HostError::Journal)?;
    let database = turso::Builder::new_local(path)
        .build()
        .await
        .map_err(|_| HostError::Journal)?;
    let connection = database.connect().map_err(|_| HostError::Journal)?;
    let mut occupancy = connection
        .query(
            "SELECT COUNT(*) FROM capacity_reservations WHERE state != 'released'",
            (),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let occupancy = occupancy
        .next()
        .await
        .map_err(|_| HostError::Journal)?
        .ok_or(HostError::Journal)?
        .get::<i64>(0)
        .map_err(|_| HostError::Journal)?;
    assert_eq!(occupancy, 0);
    let mut rows = connection
        .query(
            "SELECT state, runner_id, runner_name FROM connection_runner_jits WHERE runner_id = ?1",
            [runner.id],
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let row = rows.next().await.map_err(|_| HostError::Journal)?;
    let row = row.ok_or(HostError::Journal)?;
    assert_eq!(
        row.get::<String>(0).map_err(|_| HostError::Journal)?,
        "cleaned"
    );
    assert_eq!(
        row.get::<i64>(1).map_err(|_| HostError::Journal)?,
        runner.id
    );
    assert_eq!(
        row.get::<String>(2).map_err(|_| HostError::Journal)?,
        runner.name
    );
    assert!(rows.next().await.map_err(|_| HostError::Journal)?.is_none());
    Ok(())
}

pub(super) async fn seed_cleaned_version_ten_runner(
    path: &Path,
    intent_id: i64,
    reservation_id: i64,
    legacy_state: &str,
) -> Result<(), HostError> {
    // Seed only rows in the isolated historical V10 database created by this test.
    if intent_id <= 0
        || reservation_id <= 0
        || !matches!(
            legacy_state,
            "bound" | "start_requested" | "start_uncertain" | "ready" | "started"
        )
    {
        return Err(HostError::Journal);
    }
    let path = path.to_str().ok_or(HostError::Journal)?;
    let database = turso::Builder::new_local(path)
        .build()
        .await
        .map_err(|_| HostError::Journal)?;
    let connection = database.connect().map_err(|_| HostError::Journal)?;
    let intent_changed = connection
        .execute(
            "UPDATE connection_intents SET cleanup_proven = 1 WHERE id = ?1 AND cleanup_proven = 0",
            [intent_id],
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let reservation_changed = connection
        .execute(
            "UPDATE capacity_reservations SET state = 'released', remote_cleanup_proven = 1, local_cleanup_proven = 1 WHERE id = ?1 AND intent_id = ?2 AND state = 'reserved' AND remote_cleanup_proven = 0 AND local_cleanup_proven = 0",
            (reservation_id, intent_id),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let runner_changed = connection
        .execute(
            "UPDATE connection_runner_jits SET state = ?1 WHERE intent_id = ?2 AND state IN ('ready', 'started') AND runner_id IS NOT NULL AND runner_name IS NOT NULL",
            (legacy_state, intent_id),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    if intent_changed == 1 && reservation_changed == 1 && runner_changed == 1 {
        Ok(())
    } else {
        Err(HostError::Journal)
    }
}
