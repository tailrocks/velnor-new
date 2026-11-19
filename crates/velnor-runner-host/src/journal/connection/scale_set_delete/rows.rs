// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::error::HostError;

pub(super) struct BindingRow {
    pub(super) set: Option<(i64, String, String)>,
    pub(super) intent: Option<IntentRow>,
}

pub(super) struct IntentRow {
    pub(super) state: String,
    pub(super) ownership: String,
    pub(super) remote_id: i64,
    pub(super) remote_name: String,
    pub(super) labels: Vec<String>,
    pub(super) runner_group_id: i64,
}

impl BindingRow {
    pub(super) fn read(row: &turso::Row) -> Result<Self, HostError> {
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
