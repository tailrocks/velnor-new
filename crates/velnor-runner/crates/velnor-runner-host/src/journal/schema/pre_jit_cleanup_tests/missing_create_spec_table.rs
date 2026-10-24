use super::Scratch;
use crate::journal::Journal;

#[tokio::test]
async fn current_schema_rejects_missing_create_spec_table() -> Result<(), String> {
    let scratch = Scratch::new();
    std::fs::create_dir_all(&scratch.0).map_err(|_| "Could not create test directory")?;
    let path = scratch.database();
    drop(
        Journal::open(&path)
            .await
            .map_err(|_| "Could not create journal")?,
    );
    let path_text = path.to_str().ok_or("Journal path is not UTF-8")?;
    let database = turso::Builder::new_local(path_text)
        .build()
        .await
        .map_err(|_| "Could not open test database")?;
    let connection = database
        .connect()
        .map_err(|_| "Could not open test connection")?;
    connection
        .execute("DROP TABLE capacity_reservation_pre_jit_create_specs", ())
        .await
        .map_err(|_| "Could not remove test table")?;
    drop(connection);
    drop(database);
    if Journal::open(&path).await.is_ok() {
        return Err("The current-schema check repaired a missing v32 table".to_owned());
    }
    Ok(())
}
