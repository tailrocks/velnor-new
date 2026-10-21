use crate::journal::Journal;

#[tokio::test]
async fn version_thirty_one_upgrade_installs_only_the_typed_create_receipt() -> Result<(), String> {
    let scratch = super::Scratch::new();
    std::fs::create_dir_all(&scratch.0).map_err(|_| "The test directory could not be created.")?;
    let path = scratch.database();
    crate::journal::schema::historical_fixture::install(&path, 31)
        .await
        .map_err(|_| "The test could not install the version 31 historical fixture.")?;
    let path_text = path.to_str().ok_or("The journal path is not UTF-8.")?;
    drop(
        Journal::open(&path)
            .await
            .map_err(|_| "The journal could not be upgraded.")?,
    );
    let database = turso::Builder::new_local(path_text)
        .build()
        .await
        .map_err(|_| "The test database could not be reopened.")?;
    let connection = database
        .connect()
        .map_err(|_| "The test connection could not be reopened.")?;
    super::super::validate(&connection)
        .await
        .map_err(|_| "The upgraded schema did not validate.")?;
    let mut rows = connection
        .query(
            "SELECT COUNT(*) FROM capacity_reservation_pre_jit_create_responses",
            (),
        )
        .await
        .map_err(|_| "The create receipt count could not be read.")?;
    let count: i64 = rows
        .next()
        .await
        .map_err(|_| "The create receipt count could not be read.")?
        .ok_or("The create receipt count is missing.")?
        .get(0)
        .map_err(|_| "The create receipt count is invalid.")?;
    if count != 0 {
        return Err("The upgrade must not invent create receipts.".to_owned());
    }
    Ok(())
}

pub(super) async fn restore_v31_observer_guard(
    connection: &turso::Connection,
) -> Result<(), String> {
    let mut rows = connection
        .query(
            "SELECT sql FROM sqlite_schema WHERE type = 'trigger' AND name = 'cleanup_observer_probe_insert_guard'",
            (),
        )
        .await
        .map_err(|_| "The observer guard could not be read.")?;
    let current = rows
        .next()
        .await
        .map_err(|_| "The observer guard could not be read.")?
        .ok_or("The observer guard is absent.")?
        .get::<String>(0)
        .map_err(|_| "The observer guard is invalid.")?;
    let clause = "'reserved', 'uncertain', 'recovering'";
    if current.matches(clause).count() != 1 {
        return Err("The observer guard does not have the version 32 form.".to_owned());
    }
    let version_31 = current.replace(clause, "'reserved', 'uncertain'");
    connection
        .execute("DROP TRIGGER cleanup_observer_probe_insert_guard", ())
        .await
        .map_err(|_| "The observer guard could not be removed.")?;
    connection
        .execute(&version_31, ())
        .await
        .map_err(|_| "The observer guard could not be restored.")?;
    Ok(())
}

pub(super) async fn remove_v33_physical_attempt_schema(
    connection: &turso::Connection,
) -> Result<(), String> {
    connection
        .execute("PRAGMA foreign_keys = OFF", ())
        .await
        .map_err(|_| "The version 32 test database could not be configured.")?;
    for table in [
        "physical_attempt_outcomes",
        "physical_attempt_current",
        "physical_attempts",
        "physical_attempt_lineages",
    ] {
        for trigger in ["insert", "update", "delete"] {
            let name = format!("{table}_revision_{trigger}");
            connection
                .execute(&format!("DROP TRIGGER IF EXISTS {name}"), ())
                .await
                .map_err(|_| "Could not remove the version 33 revision guard.")?;
        }
    }
    for trigger in [
        "physical_attempt_lineages_no_update",
        "physical_attempt_lineages_no_delete",
        "physical_attempts_no_update",
        "physical_attempts_no_delete",
        "physical_attempt_insert_guard",
        "physical_attempt_outcomes_no_update",
        "physical_attempt_outcomes_no_delete",
        "physical_attempt_outcome_insert_guard",
        "physical_attempt_current_insert_guard",
        "physical_attempt_current_no_update",
        "physical_attempt_current_delete_guard",
    ] {
        connection
            .execute(&format!("DROP TRIGGER IF EXISTS {trigger}"), ())
            .await
            .map_err(|_| "Could not remove the version 33 attempt guard.")?;
    }
    for table in [
        "physical_attempt_outcomes",
        "physical_attempt_current",
        "physical_attempts",
        "physical_attempt_lineages",
    ] {
        connection
            .execute(&format!("DROP TABLE IF EXISTS {table}"), ())
            .await
            .map_err(|_| "Could not remove the version 33 attempt table.")?;
    }
    connection
        .execute("PRAGMA foreign_keys = ON", ())
        .await
        .map_err(|_| "Could not restore the test foreign keys.")?;
    Ok(())
}
