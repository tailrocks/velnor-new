// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::config::HostConfig;
use std::cmp::Ordering;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectPlan {
    Create,

    Idempotent,

    AddConnection,

    Rejected,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SetOwnership {
    Created,

    Adopted,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DisconnectEffect {
    Drain,

    DeleteSet,
}

#[must_use]
pub fn connect_plan(existing: Option<&HostConfig>, request: &HostConfig) -> ConnectPlan {
    if request.validate().is_err() || (existing.is_none() && request.connections().is_empty()) {
        return ConnectPlan::Rejected;
    }
    let Some(current) = existing else {
        return ConnectPlan::Create;
    };
    if current.validate().is_err()
        || current.host != request.host
        || current.docker != request.docker
    {
        return ConnectPlan::Rejected;
    }
    let preserved = current
        .connections()
        .iter()
        .all(|stored| request.connection(&stored.connection_id) == Some(stored));
    if !preserved {
        return ConnectPlan::Rejected;
    }
    match current
        .connections()
        .len()
        .cmp(&request.connections().len())
    {
        Ordering::Equal => ConnectPlan::Idempotent,
        Ordering::Less => ConnectPlan::AddConnection,
        Ordering::Greater => ConnectPlan::Rejected,
    }
}

#[must_use]
pub fn disconnect_effects(ownership: SetOwnership, drain: bool) -> Vec<DisconnectEffect> {
    let mut effects = Vec::new();
    if drain {
        effects.push(DisconnectEffect::Drain);
    }
    if ownership == SetOwnership::Created {
        effects.push(DisconnectEffect::DeleteSet);
    }
    effects
}
