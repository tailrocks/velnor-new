//! Connect and disconnect decisions. Adopted sets are not deleted.

use crate::config::HostConfig;
use std::cmp::Ordering;

/// Result of comparing a candidate with stored config.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectPlan {
    /// No config is stored.
    Create,
    /// All stored bindings and shared settings match.
    Idempotent,
    /// Candidate adds connections and preserves all stored bindings.
    AddConnection,
    /// Candidate changes a stored binding or shared setting.
    Rejected,
}

/// Who created the scale set.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SetOwnership {
    /// This controller created it and recorded that fact.
    Created,
    /// The set already existed. Do not delete it.
    Adopted,
}

/// Effects `disconnect` is allowed to perform.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DisconnectEffect {
    /// Stop new admissions.
    Drain,
    /// Delete the set. Only for [`SetOwnership::Created`].
    DeleteSet,
}

/// Add connections only when all stored bindings stay unchanged.
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

/// Drain when asked. Delete only a set this controller created.
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
