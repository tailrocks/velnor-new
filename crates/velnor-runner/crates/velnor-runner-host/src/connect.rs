use crate::config::HostConfig;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectPlan {
    Create,

    Idempotent,

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
    let Some(current) = existing else {
        return ConnectPlan::Create;
    };
    let same = current.github.repository == request.github.repository
        && current.github.scale_set_name == request.github.scale_set_name
        && current.docker.platform == request.docker.platform
        && current.docker.context == request.docker.context
        && current.docker.endpoint == request.docker.endpoint;
    if same {
        ConnectPlan::Idempotent
    } else {
        ConnectPlan::Rejected
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
