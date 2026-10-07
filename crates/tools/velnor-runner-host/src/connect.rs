//! Connect and disconnect decisions. Adopted sets are not deleted.

use crate::config::HostConfig;

/// Result of comparing a request with stored config.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectPlan {
    /// Nothing is stored yet.
    Create,
    /// Exact same binding, trust policy, capacity, platform, and Docker settings.
    Idempotent,
    /// Any stored configuration differs. Refused.
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

/// Idempotent only when every stored configuration field matches. Any change
/// requires an explicit disconnect and reconfiguration.
#[must_use]
pub fn connect_plan(existing: Option<&HostConfig>, request: &HostConfig) -> ConnectPlan {
    let Some(current) = existing else {
        return ConnectPlan::Create;
    };
    if current == request {
        ConnectPlan::Idempotent
    } else {
        ConnectPlan::Rejected
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

#[cfg(test)]
mod tests;
