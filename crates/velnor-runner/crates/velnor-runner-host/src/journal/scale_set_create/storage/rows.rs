use crate::connect::SetOwnership;
use crate::error::HostError;
use crate::journal::connection::ConnectionJournal;

use super::super::{
    ScaleSetCreateId, ScaleSetCreateReceipt, ScaleSetCreateRecord, ScaleSetCreateRequest,
    ScaleSetCreateState, VerifiedScaleSet,
};

pub(super) async fn write_receipt(
    connection: &turso::Connection,
    id: ScaleSetCreateId,
    observed: &VerifiedScaleSet,
    state: &str,
    prior_state: &str,
    no_effect_status: Option<u16>,
) -> Result<(), HostError> {
    let labels = serde_json::to_string(&observed.labels).map_err(|_| HostError::Journal)?;
    let changed = connection
        .execute(
            "UPDATE connection_scale_set_intents SET state = ?1, no_effect_status = ?2, remote_id = ?3, remote_name = ?4, remote_labels = ?5, disable_update = ?6, ownership = ?7 WHERE id = ?8 AND state = ?9",
            (
                state,
                no_effect_status.map(i64::from),
                observed.id,
                observed.name.as_str(),
                labels,
                bool_integer(observed.disable_update),
                if state == "created" { "created" } else { "adopted" },
                id.0,
                prior_state,
            ),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    require_one(changed)
}

pub(super) async fn insert_request(
    connection: &turso::Connection,
    request: &ScaleSetCreateRequest,
    state: &str,
    observed: Option<&VerifiedScaleSet>,
    no_effect_status: Option<u16>,
) -> Result<(), HostError> {
    let selector_labels =
        serde_json::to_string(&request.selector_labels).map_err(|_| HostError::Journal)?;
    let remote_labels = observed
        .map(|verified| serde_json::to_string(&verified.labels))
        .transpose()
        .map_err(|_| HostError::Journal)?;
    let ownership = observed.map(|_| {
        if state == "created" {
            "created"
        } else {
            "adopted"
        }
    });
    connection
        .execute(
            "INSERT INTO connection_scale_set_intents (connection_id, binding_digest, repository_id, repository, selector_name, selector_labels, runner_group_id, state, no_effect_status, remote_id, remote_name, remote_labels, disable_update, ownership) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
            (
                request.connection_id.as_str(),
                request.binding_digest.as_str(),
                request.repository_id,
                request.repository.as_str(),
                request.selector_name.as_str(),
                selector_labels,
                request.runner_group_id,
                state,
                no_effect_status.map(i64::from),
                observed.map(|verified| verified.id),
                observed.map(|verified| verified.name.as_str()),
                remote_labels,
                observed.map(|verified| bool_integer(verified.disable_update)),
                ownership,
            ),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    Ok(())
}

pub(super) async fn record(
    connection: &turso::Connection,
    request: &ScaleSetCreateRequest,
) -> Result<Option<ScaleSetCreateRecord>, HostError> {
    let mut rows = connection
        .query(
            "SELECT id, binding_digest, repository_id, repository, selector_name, selector_labels, runner_group_id, state, no_effect_status, remote_id, remote_name, remote_labels, disable_update, ownership FROM connection_scale_set_intents WHERE connection_id = ?1",
            [request.connection_id.as_str()],
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? else {
        return Ok(None);
    };
    parse_record(&row, request).map(Some)
}

fn parse_record(
    row: &turso::Row,
    request: &ScaleSetCreateRequest,
) -> Result<ScaleSetCreateRecord, HostError> {
    let id = row.get::<i64>(0).map_err(|_| HostError::Journal)?;
    let binding_digest = row.get::<String>(1).map_err(|_| HostError::Journal)?;
    let repository_id = row.get::<i64>(2).map_err(|_| HostError::Journal)?;
    let repository = row.get::<String>(3).map_err(|_| HostError::Journal)?;
    let selector_name = row.get::<String>(4).map_err(|_| HostError::Journal)?;
    let selector_labels = row.get::<String>(5).map_err(|_| HostError::Journal)?;
    let runner_group_id = row.get::<i64>(6).map_err(|_| HostError::Journal)?;
    let expected_labels =
        serde_json::to_string(&request.selector_labels).map_err(|_| HostError::Journal)?;
    if id <= 0
        || binding_digest != request.binding_digest
        || repository_id != request.repository_id
        || repository != request.repository
        || selector_name != request.selector_name
        || selector_labels != expected_labels
        || runner_group_id != request.runner_group_id
    {
        return Err(HostError::Identity);
    }
    let state_text = row.get::<String>(7).map_err(|_| HostError::Journal)?;
    let state = ScaleSetCreateState::parse(&state_text)?;
    let status = row
        .get::<Option<i64>>(8)
        .map_err(|_| HostError::Journal)?
        .map(u16::try_from)
        .transpose()
        .map_err(|_| HostError::Journal)?;
    let receipt_columns = StoredReceipt {
        status,
        remote_id: row.get::<Option<i64>>(9).map_err(|_| HostError::Journal)?,
        remote_name: row
            .get::<Option<String>>(10)
            .map_err(|_| HostError::Journal)?,
        remote_labels: row
            .get::<Option<String>>(11)
            .map_err(|_| HostError::Journal)?,
        disable_update: row.get::<Option<i64>>(12).map_err(|_| HostError::Journal)?,
        ownership: row
            .get::<Option<String>>(13)
            .map_err(|_| HostError::Journal)?,
    };
    let receipt = parse_receipt(request, state, receipt_columns)?;
    Ok(ScaleSetCreateRecord {
        id: ScaleSetCreateId(id),
        request: request.clone(),
        state,
        no_effect_status: status,
        receipt,
    })
}

fn parse_receipt(
    request: &ScaleSetCreateRequest,
    state: ScaleSetCreateState,
    columns: StoredReceipt,
) -> Result<Option<ScaleSetCreateReceipt>, HostError> {
    let StoredReceipt {
        status,
        remote_id,
        remote_name,
        remote_labels,
        disable_update,
        ownership,
    } = columns;
    if (state == ScaleSetCreateState::NoEffect && status != Some(409))
        || (!matches!(
            state,
            ScaleSetCreateState::NoEffect | ScaleSetCreateState::Adopted
        ) && status.is_some())
    {
        return Err(HostError::Journal);
    }
    let remote = match (
        remote_id,
        remote_name,
        remote_labels,
        disable_update,
        ownership,
    ) {
        (None, None, None, None, None) => None,
        (Some(id), Some(name), Some(labels), Some(disabled), Some(owner)) => {
            let labels: Vec<String> =
                serde_json::from_str(&labels).map_err(|_| HostError::Journal)?;
            let verified = VerifiedScaleSet::new(request, id, &name, &labels, disabled == 1)?;
            let parsed_owner = match owner.as_str() {
                "created" => SetOwnership::Created,
                "adopted" => SetOwnership::Adopted,
                _ => return Err(HostError::Journal),
            };
            if (state == ScaleSetCreateState::Created && parsed_owner != SetOwnership::Created)
                || (state == ScaleSetCreateState::Adopted && parsed_owner != SetOwnership::Adopted)
                || !matches!(
                    state,
                    ScaleSetCreateState::Created | ScaleSetCreateState::Adopted
                )
            {
                return Err(HostError::Journal);
            }
            Some(verified.receipt(parsed_owner))
        }
        _ => return Err(HostError::Journal),
    };
    if matches!(
        state,
        ScaleSetCreateState::Created | ScaleSetCreateState::Adopted
    ) != remote.is_some()
    {
        return Err(HostError::Journal);
    }
    Ok(remote)
}

struct StoredReceipt {
    status: Option<u16>,
    remote_id: Option<i64>,
    remote_name: Option<String>,
    remote_labels: Option<String>,
    disable_update: Option<i64>,
    ownership: Option<String>,
}

pub(super) async fn bind_runtime_scale_set(
    connection: &turso::Connection,
    scope: &ConnectionJournal,
    observed: &VerifiedScaleSet,
    ownership: SetOwnership,
) -> Result<(), HostError> {
    connection
        .execute(
            "INSERT OR IGNORE INTO connection_runtime_bindings (connection_id) VALUES (?1)",
            [scope.connection_id().as_str()],
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let ownership = match ownership {
        SetOwnership::Created => "created",
        SetOwnership::Adopted => "adopted",
    };
    let changed = connection
        .execute(
            "UPDATE connection_runtime_bindings SET scale_set_id = COALESCE(scale_set_id, ?1), scale_set_name = COALESCE(scale_set_name, ?2), set_ownership = COALESCE(set_ownership, ?3) WHERE connection_id = ?4 AND (scale_set_id IS NULL OR scale_set_id = ?1) AND (scale_set_name IS NULL OR scale_set_name = ?2) AND (set_ownership IS NULL OR set_ownership = ?3)",
            (
                observed.id,
                observed.name.as_str(),
                ownership,
                scope.connection_id().as_str(),
            ),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    if changed == 1 {
        return Ok(());
    }
    let mut rows = connection
        .query(
            "SELECT scale_set_id, scale_set_name, set_ownership FROM connection_runtime_bindings WHERE connection_id = ?1",
            [scope.connection_id().as_str()],
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let row = rows.next().await.map_err(|_| HostError::Journal)?;
    let Some(row) = row else {
        return Err(HostError::Journal);
    };
    let id = row.get::<Option<i64>>(0).map_err(|_| HostError::Journal)?;
    let name = row
        .get::<Option<String>>(1)
        .map_err(|_| HostError::Journal)?;
    let stored_owner = row
        .get::<Option<String>>(2)
        .map_err(|_| HostError::Journal)?;
    if id == Some(observed.id)
        && name.as_deref() == Some(observed.name.as_str())
        && stored_owner.as_deref() == Some(ownership)
    {
        Ok(())
    } else {
        Err(HostError::Identity)
    }
}

pub(super) fn same_remote(receipt: &ScaleSetCreateReceipt, observed: &VerifiedScaleSet) -> bool {
    let mut recorded_labels = receipt.labels.clone();
    let mut observed_labels = observed.labels.clone();
    recorded_labels.sort_unstable();
    observed_labels.sort_unstable();
    receipt.id == observed.id
        && receipt.name == observed.name
        && recorded_labels == observed_labels
        && receipt.disable_update == observed.disable_update
}

pub(super) fn require_one(changed: u64) -> Result<(), HostError> {
    if changed == 1 {
        Ok(())
    } else {
        Err(HostError::Journal)
    }
}

const fn bool_integer(value: bool) -> i64 {
    if value { 1 } else { 0 }
}
