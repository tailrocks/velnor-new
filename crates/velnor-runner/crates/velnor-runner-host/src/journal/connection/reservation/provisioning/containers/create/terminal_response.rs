use crate::error::HostError;
use crate::journal::connection::ConnectionJournal;
use crate::launch::bootstrap::BootstrapContainerRole;

use super::super::BootstrapContainerCreateTerminalResponse;

impl ConnectionJournal {
    /// Read the terminal daemon reply only while its original pre-JIT hold is unresolved.
    pub(crate) async fn bootstrap_container_create_terminal_response(
        &self,
        reservation_id: i64,
        role: BootstrapContainerRole,
    ) -> Result<Option<BootstrapContainerCreateTerminalResponse>, HostError> {
        if reservation_id <= 0 {
            return Err(HostError::Identity);
        }
        let connection_id = self.connection_id().as_str().to_owned();
        super::in_transaction(self.journal_ref(), async |connection| {
            self.validate_scope(connection).await?;
            let Some(response) = read_terminal_response(connection, reservation_id, role).await?
            else {
                return Ok(None);
            };
            validate_terminal_response(connection, &connection_id, &response).await?;
            Ok(Some(response))
        })
        .await
    }
}

async fn read_terminal_response(
    connection: &turso::Connection,
    reservation_id: i64,
    role: BootstrapContainerRole,
) -> Result<Option<BootstrapContainerCreateTerminalResponse>, HostError> {
    let mut rows = connection
        .query(
            "SELECT connection_id, operation_id, engine_id, docker_root_digest, container_name, client_api_major, client_api_minor, response_status FROM capacity_reservation_pre_jit_create_responses WHERE reservation_id = ?1 AND role = ?2",
            (reservation_id, super::container_role(role)),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? else {
        return Ok(None);
    };
    let response = BootstrapContainerCreateTerminalResponse {
        connection_id: row.get(0).map_err(|_| HostError::Journal)?,
        reservation_id,
        operation_id: row.get(1).map_err(|_| HostError::Journal)?,
        engine_id: row.get(2).map_err(|_| HostError::Journal)?,
        docker_root_digest: row.get(3).map_err(|_| HostError::Journal)?,
        container_name: row.get(4).map_err(|_| HostError::Journal)?,
        client_api_major: checked_u16(row.get::<i64>(5).map_err(|_| HostError::Journal)?)?,
        client_api_minor: checked_u16(row.get::<i64>(6).map_err(|_| HostError::Journal)?)?,
        status_code: checked_u16(row.get::<i64>(7).map_err(|_| HostError::Journal)?)?,
        role,
    };
    if rows.next().await.map_err(|_| HostError::Journal)?.is_some() {
        return Err(HostError::Journal);
    }
    Ok(Some(response))
}

fn checked_u16(value: i64) -> Result<u16, HostError> {
    u16::try_from(value).map_err(|_| HostError::Journal)
}

async fn validate_terminal_response(
    connection: &turso::Connection,
    connection_id: &str,
    response: &BootstrapContainerCreateTerminalResponse,
) -> Result<(), HostError> {
    let Some(context) =
        super::hold_context(connection, connection_id, response.reservation_id()).await?
    else {
        return Err(HostError::Journal);
    };
    if response.connection_id() != connection_id
        || context.reservation_state != "uncertain"
        || context.intent_state != "uncertain"
        || context.cleanup_proven
        || context.operation_id != response.operation_id()
        || context.engine_id != response.engine_id()
        || context.docker_root_digest != response.docker_root_digest()
        || response.client_api_version() != (1, 53)
        || !matches!(response.status_code(), 400 | 404 | 409)
    {
        return Err(HostError::Journal);
    }
    validate_unstarted_container(connection, response).await?;
    validate_unbound_state(connection, response.reservation_id()).await
}

async fn validate_unstarted_container(
    connection: &turso::Connection,
    response: &BootstrapContainerCreateTerminalResponse,
) -> Result<(), HostError> {
    let mut rows = connection
        .query(
            "SELECT container_name, create_state, container_id, start_state FROM capacity_reservation_bootstrap_containers WHERE reservation_id = ?1 AND role = ?2",
            (
                response.reservation_id(),
                super::container_role(response.role()),
            ),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let row = rows
        .next()
        .await
        .map_err(|_| HostError::Journal)?
        .ok_or(HostError::Journal)?;
    let name: String = row.get(0).map_err(|_| HostError::Journal)?;
    let create_state: String = row.get(1).map_err(|_| HostError::Journal)?;
    let container_id: Option<String> = row.get(2).map_err(|_| HostError::Journal)?;
    let start_state: String = row.get(3).map_err(|_| HostError::Journal)?;
    if rows.next().await.map_err(|_| HostError::Journal)?.is_some()
        || name != response.container_name()
        || create_state != "uncertain"
        || container_id.is_some()
        || start_state != "not_requested"
    {
        return Err(HostError::Journal);
    }
    Ok(())
}

async fn validate_unbound_state(
    connection: &turso::Connection,
    reservation_id: i64,
) -> Result<(), HostError> {
    let mut rows = connection
        .query(
            "SELECT bootstrap_start_state, resources_bound, jit_state, publication_state, delivery_state, storage_cleanup_proven FROM capacity_reservation_provisioning_state WHERE reservation_id = ?1",
            [reservation_id],
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let row = rows
        .next()
        .await
        .map_err(|_| HostError::Journal)?
        .ok_or(HostError::Journal)?;
    let bootstrap_start_state: String = row.get(0).map_err(|_| HostError::Journal)?;
    let resources_bound: i64 = row.get(1).map_err(|_| HostError::Journal)?;
    let jit_state: String = row.get(2).map_err(|_| HostError::Journal)?;
    let publication_state: String = row.get(3).map_err(|_| HostError::Journal)?;
    let delivery_state: String = row.get(4).map_err(|_| HostError::Journal)?;
    let cleanup_proven: i64 = row.get(5).map_err(|_| HostError::Journal)?;
    if rows.next().await.map_err(|_| HostError::Journal)?.is_some()
        || bootstrap_start_state != "not_requested"
        || resources_bound != 0
        || jit_state != "not_requested"
        || publication_state != "not_requested"
        || delivery_state != "not_requested"
        || cleanup_proven != 0
    {
        return Err(HostError::Journal);
    }
    Ok(())
}
