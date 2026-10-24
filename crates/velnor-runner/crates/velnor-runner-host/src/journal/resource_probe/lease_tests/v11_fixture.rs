//! This fixture holds data from a version 11 journal.
//!
//! The source commit is `afb7ab57c2cf56ab79e99e7530678f439e048914`.
//! The test used `Journal::open` from the earlier source.
//! It set capacity.
//! It recorded a `resource-probe` operation in phase `create_requested`.
//! It saved the SQL schema and row data.

use std::path::Path;

use crate::error::HostError;

const FIXTURE_SQL: &str = include_str!("v11_fixture.sql");

pub(super) const INSTANCE_ID: &str = "ff6daf88b2f34ecda4ef46ab3be1d14f";
pub(super) const OPERATION_ID: &str = "dddddddddddddddddddddddddddddddd";
pub(super) const ENGINE_ID: &str = "probe-engine";
pub(super) const DOCKER_ROOT_DIGEST: &str =
    "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
pub(super) const OPERATION_NAME: &str = "velnor-resource-probe-dddddddddddddddddddddddddddddddd";

pub(super) async fn seed(path: &Path) -> Result<(), HostError> {
    let path = path.to_str().ok_or(HostError::Path)?;
    let database = turso::Builder::new_local(path)
        .build()
        .await
        .map_err(|_| HostError::Journal)?;
    let connection = database.connect().map_err(|_| HostError::Journal)?;
    for statement in FIXTURE_SQL.split(";\n").map(str::trim) {
        if !statement.is_empty() {
            connection
                .execute(statement, ())
                .await
                .map_err(|_| HostError::Journal)?;
        }
    }
    Ok(())
}
