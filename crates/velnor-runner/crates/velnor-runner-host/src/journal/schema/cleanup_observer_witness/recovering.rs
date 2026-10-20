use super::super::sql_normalization::normalize;
use super::PROBE_INSERT_GUARD;
use crate::error::HostError;

const V31_STATE: &str = "reservation.state IN ('reserved', 'uncertain')";
const V32_STATE: &str = "reservation.state IN ('reserved', 'uncertain', 'recovering')";

pub(in crate::journal::schema) async fn upgrade_for_v32(
    connection: &turso::Connection,
) -> Result<(), HostError> {
    let actual = trigger_sql(connection).await?.ok_or(HostError::Journal)?;
    if normalize(&actual) != normalize(PROBE_INSERT_GUARD) {
        return Err(HostError::Journal);
    }
    let replacement = v32_probe_insert_guard()?;
    connection
        .execute("DROP TRIGGER cleanup_observer_probe_insert_guard", ())
        .await
        .map_err(|_| HostError::Journal)?;
    connection
        .execute(&replacement, ())
        .await
        .map_err(|_| HostError::Journal)?;
    let installed = trigger_sql(connection).await?.ok_or(HostError::Journal)?;
    if normalize(&installed) == normalize(&replacement) {
        Ok(())
    } else {
        Err(HostError::Journal)
    }
}

pub(super) fn is_v32_probe_insert_guard(actual: &str) -> Result<bool, HostError> {
    Ok(normalize(actual) == normalize(&v32_probe_insert_guard()?))
}

pub(in crate::journal::schema) async fn validate_current(
    connection: &turso::Connection,
) -> Result<(), HostError> {
    super::validate(connection).await?;
    let actual = trigger_sql(connection).await?.ok_or(HostError::Journal)?;
    if is_v32_probe_insert_guard(&actual)? {
        Ok(())
    } else {
        Err(HostError::Journal)
    }
}

fn v32_probe_insert_guard() -> Result<String, HostError> {
    if PROBE_INSERT_GUARD.matches(V31_STATE).count() != 1 {
        return Err(HostError::Journal);
    }
    Ok(PROBE_INSERT_GUARD.replace(V31_STATE, V32_STATE))
}

async fn trigger_sql(connection: &turso::Connection) -> Result<Option<String>, HostError> {
    let mut rows = connection
        .query(
            "SELECT sql FROM sqlite_schema WHERE type = 'trigger' AND name = 'cleanup_observer_probe_insert_guard'",
            (),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? else {
        return Ok(None);
    };
    row.get(0).map(Some).map_err(|_| HostError::Journal)
}
