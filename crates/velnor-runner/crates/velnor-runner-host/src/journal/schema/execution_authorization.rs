//! Durable schema for execution-authority snapshots and decisions.

mod ddl;
mod validation;

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

pub(super) async fn upgrade_v28(
    connection: &turso::Connection,
) -> Result<(), crate::error::HostError> {
    ddl::upgrade_v28(connection).await
}
