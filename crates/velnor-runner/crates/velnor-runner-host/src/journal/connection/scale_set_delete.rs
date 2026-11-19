// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::error::HostError;
use crate::scale_set::delete::ScaleSetAbsence;

use super::ConnectionJournal;
use super::storage::{
    has_ambiguous_legacy_work, has_connection_work, in_transaction, read_transaction,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ScaleSetDeleteState {
    Prepared,

    Requested,

    Uncertain,

    Deleted,
}

impl ScaleSetDeleteState {
    fn parse(value: &str) -> Result<Self, HostError> {
        match value {
            "prepared" => Ok(Self::Prepared),
            "requested" => Ok(Self::Requested),
            "uncertain" => Ok(Self::Uncertain),
            "deleted" => Ok(Self::Deleted),
            _ => Err(HostError::Journal),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ScaleSetDeleteTarget {
    pub(crate) scale_set_id: i64,

    pub(crate) name: String,

    pub(crate) labels: Vec<String>,

    pub(crate) runner_group_id: i64,

    pub(crate) state: Option<ScaleSetDeleteState>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ScaleSetDeleteGate {
    NotOwned,

    NotDrained,

    Deleted,

    Owned(ScaleSetDeleteTarget),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ScaleSetDeleteMode {
    Send,

    Observe,

    Done,
}

#[derive(Debug)]
pub(crate) struct ScaleSetDeleteClaim {
    scale_set_id: i64,
    mode: ScaleSetDeleteMode,
}

impl ScaleSetDeleteClaim {
    #[must_use]
    pub(crate) const fn mode(&self) -> ScaleSetDeleteMode {
        self.mode
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ScaleSetDeleted(());

impl ScaleSetDeleted {
    pub(crate) const fn after_http_204() -> Self {
        Self(())
    }

    pub(crate) const fn after_proven_absence(_proof: &ScaleSetAbsence) -> Self {
        Self(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ScaleSetDeleteRecord {
    Deleted(ScaleSetDeleted),

    Refused,

    Uncertain,

    Resend,
}

impl ConnectionJournal {
    pub(crate) async fn scale_set_delete_gate(&self) -> Result<ScaleSetDeleteGate, HostError> {
        read_transaction(&self.journal, async |connection| {
            self.delete_gate_in(connection).await
        })
        .await
    }

    pub(crate) async fn claim_scale_set_delete(
        &self,
        target: &ScaleSetDeleteTarget,
    ) -> Result<ScaleSetDeleteClaim, HostError> {
        in_transaction(&self.journal, async |connection| {
            let ScaleSetDeleteGate::Owned(current) = self.delete_gate_in(connection).await? else {
                return Err(HostError::Identity);
            };
            if current.scale_set_id != target.scale_set_id
                || current.runner_group_id != target.runner_group_id
            {
                return Err(HostError::Identity);
            }
            let mode = match current.state {
                None => {
                    insert_row(connection, self.connection_id.as_str(), &current).await?;
                    move_state(connection, self, &["prepared"], "requested").await?;
                    ScaleSetDeleteMode::Send
                }
                Some(ScaleSetDeleteState::Prepared) => {
                    move_state(connection, self, &["prepared"], "requested").await?;
                    ScaleSetDeleteMode::Send
                }
                Some(ScaleSetDeleteState::Requested | ScaleSetDeleteState::Uncertain) => {
                    ScaleSetDeleteMode::Observe
                }
                Some(ScaleSetDeleteState::Deleted) => ScaleSetDeleteMode::Done,
            };
            Ok(ScaleSetDeleteClaim {
                scale_set_id: current.scale_set_id,
                mode,
            })
        })
        .await
    }

    pub(crate) async fn record_scale_set_delete(
        &self,
        claim: &ScaleSetDeleteClaim,
        record: ScaleSetDeleteRecord,
    ) -> Result<(), HostError> {
        in_transaction(&self.journal, async |connection| {
            self.validate_scope(connection).await?;
            if stored_scale_set_id(connection, self.connection_id.as_str()).await?
                != Some(claim.scale_set_id)
            {
                return Err(HostError::Identity);
            }
            let (from, to) = match record {
                ScaleSetDeleteRecord::Deleted(_) => (&["requested", "uncertain"][..], "deleted"),
                ScaleSetDeleteRecord::Refused => (&["requested"][..], "prepared"),
                ScaleSetDeleteRecord::Uncertain => (&["requested", "uncertain"][..], "uncertain"),
                ScaleSetDeleteRecord::Resend => (&["requested", "uncertain"][..], "requested"),
            };
            move_state(connection, self, from, to).await
        })
        .await
    }

    async fn delete_gate_in(
        &self,
        connection: &turso::Connection,
    ) -> Result<ScaleSetDeleteGate, HostError> {
        self.validate_scope(connection).await?;
        let id = self.connection_id.as_str();
        if has_ambiguous_legacy_work(connection).await?
            || has_connection_work(connection, id).await?
        {
            return Ok(ScaleSetDeleteGate::NotDrained);
        }
        let mut rows = connection
            .query(
                "SELECT b.scale_set_id, b.scale_set_name, b.set_ownership, i.state, i.ownership, i.remote_id, i.remote_name, i.remote_labels, i.runner_group_id FROM connection_runtime_bindings AS b LEFT JOIN connection_scale_set_intents AS i ON i.connection_id = b.connection_id WHERE b.connection_id = ?1",
                [id],
            )
            .await
            .map_err(|_| HostError::Journal)?;
        let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? else {
            return Ok(ScaleSetDeleteGate::NotOwned);
        };
        let binding = BindingRow::read(&row)?;
        drop(rows);
        let Some((bound_id, bound_name, bound_owner)) = binding.set else {
            return Ok(ScaleSetDeleteGate::NotOwned);
        };
        let intent = binding.intent.ok_or(HostError::Identity)?;
        if intent.remote_id != bound_id
            || intent.remote_name != bound_name
            || intent.ownership != bound_owner
            || intent.state != bound_owner
        {
            return Err(HostError::Identity);
        }
        if bound_owner != "created" {
            return Ok(ScaleSetDeleteGate::NotOwned);
        }
        let state = stored_state(connection, id).await?;
        if state == Some(ScaleSetDeleteState::Deleted) {
            return Ok(ScaleSetDeleteGate::Deleted);
        }
        Ok(ScaleSetDeleteGate::Owned(ScaleSetDeleteTarget {
            scale_set_id: bound_id,
            name: bound_name,
            labels: intent.labels,
            runner_group_id: intent.runner_group_id,
            state,
        }))
    }
}

struct BindingRow {
    set: Option<(i64, String, String)>,
    intent: Option<IntentRow>,
}

struct IntentRow {
    state: String,
    ownership: String,
    remote_id: i64,
    remote_name: String,
    labels: Vec<String>,
    runner_group_id: i64,
}

impl BindingRow {
    fn read(row: &turso::Row) -> Result<Self, HostError> {
        let id = row.get::<Option<i64>>(0).map_err(|_| HostError::Journal)?;
        let name = row
            .get::<Option<String>>(1)
            .map_err(|_| HostError::Journal)?;
        let owner = row
            .get::<Option<String>>(2)
            .map_err(|_| HostError::Journal)?;
        let set = match (id, name, owner) {
            (None, None, None) => None,
            (Some(id), Some(name), Some(owner)) if id > 0 && !name.is_empty() => {
                Some((id, name, owner))
            }
            _ => return Err(HostError::Journal),
        };
        let state = row
            .get::<Option<String>>(3)
            .map_err(|_| HostError::Journal)?;
        let intent = match state {
            None => None,
            Some(state) => Some(IntentRow::read(row, state)?),
        };
        Ok(Self { set, intent })
    }
}

impl IntentRow {
    fn read(row: &turso::Row, state: String) -> Result<Self, HostError> {
        let labels = row
            .get::<Option<String>>(7)
            .map_err(|_| HostError::Journal)?;
        Ok(Self {
            state,
            ownership: row
                .get::<Option<String>>(4)
                .map_err(|_| HostError::Journal)?
                .unwrap_or_default(),
            remote_id: row
                .get::<Option<i64>>(5)
                .map_err(|_| HostError::Journal)?
                .unwrap_or_default(),
            remote_name: row
                .get::<Option<String>>(6)
                .map_err(|_| HostError::Journal)?
                .unwrap_or_default(),
            labels: labels
                .map(|text| serde_json::from_str(&text))
                .transpose()
                .map_err(|_| HostError::Journal)?
                .unwrap_or_default(),
            runner_group_id: row.get::<i64>(8).map_err(|_| HostError::Journal)?,
        })
    }
}

async fn stored_state(
    connection: &turso::Connection,
    connection_id: &str,
) -> Result<Option<ScaleSetDeleteState>, HostError> {
    let mut rows = connection
        .query(
            "SELECT state FROM connection_scale_set_deletes WHERE connection_id = ?1",
            [connection_id],
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? else {
        return Ok(None);
    };
    let text = row.get::<String>(0).map_err(|_| HostError::Journal)?;
    ScaleSetDeleteState::parse(&text).map(Some)
}

async fn stored_scale_set_id(
    connection: &turso::Connection,
    connection_id: &str,
) -> Result<Option<i64>, HostError> {
    let mut rows = connection
        .query(
            "SELECT scale_set_id FROM connection_scale_set_deletes WHERE connection_id = ?1",
            [connection_id],
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? else {
        return Ok(None);
    };
    row.get::<i64>(0).map(Some).map_err(|_| HostError::Journal)
}

async fn insert_row(
    connection: &turso::Connection,
    connection_id: &str,
    target: &ScaleSetDeleteTarget,
) -> Result<(), HostError> {
    connection
        .execute(
            "INSERT INTO connection_scale_set_deletes (connection_id, scale_set_id, runner_group_id, state) VALUES (?1, ?2, ?3, 'prepared')",
            (connection_id, target.scale_set_id, target.runner_group_id),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    Ok(())
}

async fn move_state(
    connection: &turso::Connection,
    scope: &ConnectionJournal,
    from: &[&str],
    to: &str,
) -> Result<(), HostError> {
    let allowed = from
        .iter()
        .map(|state| format!("'{state}'"))
        .collect::<Vec<_>>()
        .join(", ");
    let changed = connection
        .execute(
            &format!(
                "UPDATE connection_scale_set_deletes SET state = ?1 WHERE connection_id = ?2 AND state IN ({allowed})"
            ),
            (to, scope.connection_id.as_str()),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    if changed == 1 {
        Ok(())
    } else {
        Err(HostError::Journal)
    }
}

pub(super) async fn has_pending_scale_set_delete(
    connection: &turso::Connection,
    connection_id: &str,
) -> Result<bool, HostError> {
    Ok(matches!(
        stored_state(connection, connection_id).await?,
        Some(ScaleSetDeleteState::Requested | ScaleSetDeleteState::Uncertain)
    ))
}
