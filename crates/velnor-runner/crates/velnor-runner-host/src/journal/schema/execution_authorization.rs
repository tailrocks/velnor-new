// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

mod ddl;
mod validation;

#[cfg(test)]
#[path = "execution_authorization/tests.rs"]
mod tests;

pub(super) async fn bootstrap(
    connection: &turso::Connection,
) -> Result<(), crate::error::HostError> {
    ddl::bootstrap(connection).await
}

pub(super) async fn validate(
    connection: &turso::Connection,
) -> Result<(), crate::error::HostError> {
    ddl::validate_objects(connection).await?;
    validation::validate_rows(connection).await
}

pub(super) async fn validate_v53(
    connection: &turso::Connection,
) -> Result<(), crate::error::HostError> {
    ddl::validate_current_objects(connection).await?;
    validation::validate_rows(connection).await
}

pub(super) async fn upgrade_v28(
    connection: &turso::Connection,
) -> Result<(), crate::error::HostError> {
    ddl::upgrade_v28(connection).await
}

pub(super) async fn migrate_v52_to_v53(
    connection: &turso::Connection,
) -> Result<(), crate::error::HostError> {
    super::validate_persisted_schema_v52(connection).await?;
    ddl::upgrade_v53(connection).await?;
    connection
        .execute("PRAGMA user_version = 53", ())
        .await
        .map_err(|_| crate::error::HostError::Journal)?;
    super::validate_persisted_schema_v53(connection).await
}
