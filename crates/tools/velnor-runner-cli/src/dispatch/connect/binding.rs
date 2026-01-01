use super::{ConnectError, ConnectRequest};

pub(super) struct BindingAndTrust<'a> {
    pub(super) group: Option<(i64, &'a str)>,
    pub(super) explicit_binding: bool,
}

pub(super) fn validate_binding_and_trust<'a>(
    request: &ConnectRequest<'a>,
    linux: bool,
) -> Result<BindingAndTrust<'a>, ConnectError> {
    let group = match (request.runner_group_id, request.runner_group_name) {
        (Some(id), Some(name))
            if id > 0 && !name.trim().is_empty() && !name.chars().any(char::is_control) =>
        {
            Some((id, name))
        }
        (None, None) => None,
        _ => return Err(ConnectError::Config),
    };
    if request
        .registration_scope
        .is_some_and(|scope| scope != "repository")
    {
        return Err(ConnectError::Config);
    }
    let explicit_binding = request.registration_scope.is_some() || group.is_some();
    if group.is_none()
        && (request.registration_scope.is_some()
            || !request.allowed_events.is_empty()
            || !request.allowed_workflow_paths.is_empty())
    {
        return Err(ConnectError::Config);
    }
    if linux
        && (request.registration_scope != Some("repository")
            || group.is_none()
            || request.allowed_events.is_empty()
            || request.allowed_workflow_paths.is_empty()
            || request.max_jobs.is_none()
            || request.drain_timeout_secs.is_none())
    {
        return Err(ConnectError::Config);
    }
    if request.allowed_events.is_empty() != request.allowed_workflow_paths.is_empty()
        || request
            .allowed_workflow_paths
            .iter()
            .any(|path| path.is_empty() || path.chars().any(char::is_control))
    {
        return Err(ConnectError::Config);
    }
    Ok(BindingAndTrust {
        group,
        explicit_binding,
    })
}
