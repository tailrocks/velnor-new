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
