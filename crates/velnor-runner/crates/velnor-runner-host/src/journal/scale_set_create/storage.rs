//! Durable SQL operations for Scale Set create intents.

use crate::connect::SetOwnership;
use crate::error::HostError;
use crate::journal::connection::ConnectionJournal;
use crate::journal::connection::storage::in_transaction;

use super::{
    CreatedScaleSetResponse, ExistingScaleSetObservation, ScaleSetCreateClaim, ScaleSetCreateId,
    ScaleSetCreateNoEffect, ScaleSetCreateRecord, ScaleSetCreateRequest, ScaleSetCreateState,
};

mod rows;
use rows::{
    bind_runtime_scale_set, insert_request, record, require_one, same_remote, write_receipt,
};

pub(super) async fn prepare(
    scope: &ConnectionJournal,
    request: &ScaleSetCreateRequest,
) -> Result<ScaleSetCreateRecord, HostError> {
    in_transaction(scope.journal_ref(), async |connection| {
        verify_scope(scope, connection, request).await?;
        if let Some(record) = record(connection, request).await? {
            return Ok(record);
        }
        insert_request(connection, request, "prepared", None, None).await?;
        record(connection, request).await?.ok_or(HostError::Journal)
    })
    .await
}

pub(super) async fn claim(
    scope: &ConnectionJournal,
    prepared: &ScaleSetCreateRecord,
) -> Result<Option<ScaleSetCreateClaim>, HostError> {
    in_transaction(scope.journal_ref(), async |connection| {
        let request = &prepared.request;
        verify_scope(scope, connection, request).await?;
        let Some(current) = record(connection, request).await? else {
            return Err(HostError::Journal);
        };
        if current.id != prepared.id {
            return Err(HostError::Identity);
        }
        if current.state != ScaleSetCreateState::Prepared {
            return Ok(None);
        }
        let changed = connection
            .execute(
                "UPDATE connection_scale_set_intents SET state = 'requested' WHERE id = ?1 AND state = 'prepared'",
                [current.id.0],
            )
            .await
            .map_err(|_| HostError::Journal)?;
        if changed != 1 {
            return Err(HostError::Journal);
        }
        Ok(Some(ScaleSetCreateClaim::new(scope, &current)))
    })
    .await
}

pub(super) async fn mark_uncertain(
    scope: &ConnectionJournal,
    claim: ScaleSetCreateClaim,
) -> Result<ScaleSetCreateRecord, HostError> {
    let request = verify_claim(scope, &claim)?;
    in_transaction(scope.journal_ref(), async |connection| {
        verify_scope(scope, connection, request).await?;
        let current = require_row(connection, request, claim.id).await?;
        require_state(&current, ScaleSetCreateState::Requested)?;
        update_state(connection, current.id, "uncertain").await?;
        record(connection, request).await?.ok_or(HostError::Journal)
    })
    .await
}

pub(super) async fn record_no_effect(
    scope: &ConnectionJournal,
    claim: ScaleSetCreateClaim,
    proof: ScaleSetCreateNoEffect,
) -> Result<ScaleSetCreateRecord, HostError> {
    let request = verify_claim(scope, &claim)?;
    in_transaction(scope.journal_ref(), async |connection| {
        verify_scope(scope, connection, request).await?;
        let current = require_row(connection, request, claim.id).await?;
        require_state(&current, ScaleSetCreateState::Requested)?;
        let changed = connection
            .execute(
                "UPDATE connection_scale_set_intents SET state = 'no_effect', no_effect_status = ?1 WHERE id = ?2 AND state = 'requested'",
                (i64::from(proof.0), current.id.0),
            )
            .await
            .map_err(|_| HostError::Journal)?;
        require_one(changed)?;
        record(connection, request).await?.ok_or(HostError::Journal)
    })
    .await
}

pub(super) async fn record_created(
    scope: &ConnectionJournal,
    claim: ScaleSetCreateClaim,
    response: CreatedScaleSetResponse,
) -> Result<ScaleSetCreateRecord, HostError> {
    let request = verify_claim(scope, &claim)?;
    if response.0.request != *request {
        return Err(HostError::Identity);
    }
    in_transaction(scope.journal_ref(), async |connection| {
        verify_scope(scope, connection, request).await?;
        let current = require_row(connection, request, claim.id).await?;
        require_state(&current, ScaleSetCreateState::Requested)?;
        let observed = response.0;
        write_receipt(
            connection,
            current.id,
            &observed,
            "created",
            "requested",
            None,
        )
        .await?;
        bind_runtime_scale_set(connection, scope, &observed, SetOwnership::Created).await?;
        record(connection, request).await?.ok_or(HostError::Journal)
    })
    .await
}

pub(super) async fn adopt(
    scope: &ConnectionJournal,
    request: &ScaleSetCreateRequest,
    observed: ExistingScaleSetObservation,
) -> Result<ScaleSetCreateRecord, HostError> {
    if observed.0.request != *request {
        return Err(HostError::Identity);
    }
    in_transaction(scope.journal_ref(), async |connection| {
        verify_scope(scope, connection, request).await?;
        let current = record(connection, request).await?;
        if let Some(current) = current {
            match current.state {
                ScaleSetCreateState::Requested | ScaleSetCreateState::Uncertain => {
                    return Err(HostError::Journal);
                }
                ScaleSetCreateState::Created | ScaleSetCreateState::Adopted => {
                    let receipt = current.receipt.as_ref().ok_or(HostError::Journal)?;
                    if !same_remote(receipt, &observed.0) {
                        return Err(HostError::Identity);
                    }
                    return Ok(current);
                }
                ScaleSetCreateState::Prepared | ScaleSetCreateState::NoEffect => {
                    write_receipt(
                        connection,
                        current.id,
                        &observed.0,
                        "adopted",
                        current.state.as_str(),
                        current.no_effect_status,
                    )
                    .await?;
                }
            }
        } else {
            insert_request(connection, request, "adopted", Some(&observed.0), None).await?;
        }
        bind_runtime_scale_set(connection, scope, &observed.0, SetOwnership::Adopted).await?;
        record(connection, request).await?.ok_or(HostError::Journal)
    })
    .await
}

fn verify_claim<'a>(
    scope: &ConnectionJournal,
    claim: &'a ScaleSetCreateClaim,
) -> Result<&'a ScaleSetCreateRequest, HostError> {
    if claim.journal != *scope.journal_ref()
        || claim.request.connection_id != *scope.connection_id()
        || claim.request.binding_digest != scope.binding_digest()
    {
        return Err(HostError::Identity);
    }
    Ok(&claim.request)
}

async fn verify_scope(
    scope: &ConnectionJournal,
    connection: &turso::Connection,
    request: &ScaleSetCreateRequest,
) -> Result<(), HostError> {
    request.validate()?;
    if request.connection_id != *scope.connection_id()
        || request.binding_digest != scope.binding_digest()
    {
        return Err(HostError::Identity);
    }
    scope.validate_scope(connection).await
}

async fn require_row(
    connection: &turso::Connection,
    request: &ScaleSetCreateRequest,
    id: ScaleSetCreateId,
) -> Result<ScaleSetCreateRecord, HostError> {
    let current = record(connection, request)
        .await?
        .ok_or(HostError::Journal)?;
    if current.id == id {
        Ok(current)
    } else {
        Err(HostError::Identity)
    }
}

fn require_state(
    record: &ScaleSetCreateRecord,
    state: ScaleSetCreateState,
) -> Result<(), HostError> {
    if record.state == state {
        Ok(())
    } else {
        Err(HostError::Journal)
    }
}

async fn update_state(
    connection: &turso::Connection,
    id: ScaleSetCreateId,
    state: &str,
) -> Result<(), HostError> {
    let changed = connection
        .execute(
            "UPDATE connection_scale_set_intents SET state = ?1 WHERE id = ?2 AND state = 'requested'",
            (state, id.0),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    require_one(changed)
}
