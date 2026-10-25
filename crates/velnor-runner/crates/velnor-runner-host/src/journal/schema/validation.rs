use crate::error::HostError;

pub(in crate::journal) async fn validate_persisted_schema_v35(
    conn: &turso::Connection,
) -> Result<(), HostError> {
    super::legacy_validation::validate_persisted_schema_v32_core(conn).await?;
    super::storage_network::validate_v33(conn).await?;
    super::physical_attempts::validate_v35(conn).await?;
    super::post_jit_retirement::validate(conn).await
}
