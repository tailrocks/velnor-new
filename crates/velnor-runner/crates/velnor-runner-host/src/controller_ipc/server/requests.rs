use std::path::Path;

use crate::{
    error::HostError,
    journal::{DoctorProbeStartResult, DoctorProbeState, Journal},
};

use super::super::wire::{DoctorProbeReply, Request, Response, Target};
use super::{
    ControlState, configured_exactly, connection_id_present, target_marker,
    transport::{marker_exists, read_drain_token},
};

pub(super) async fn handle(
    journal: &Journal,
    state: &ControlState,
    state_path: &Path,
    request: Request,
    engine_id: &str,
    docker_root_digest: &str,
) -> Result<Response, HostError> {
    match request {
        Request::DrainStatus { target, token } => {
            drain_status(journal, state, state_path, target, token).await
        }
        Request::Reactivate { binding, token } => {
            reactivate(journal, state, state_path, binding, token).await
        }
        Request::Detached { binding, token } => {
            detached(journal, state, state_path, binding, token).await
        }
        Request::DoctorProbeStart { nonce } => {
            doctor_probe_start(journal, &nonce, engine_id, docker_root_digest).await
        }
        Request::DoctorProbeStatus { nonce } => doctor_probe_status(journal, state, &nonce).await,
    }
}

async fn drain_status(
    journal: &Journal,
    state: &ControlState,
    state_path: &Path,
    target: Target,
    token: String,
) -> Result<Response, HostError> {
    let marker = target_marker(state_path, &target);
    if read_drain_token(&marker).as_deref() != Some(token.as_str()) {
        return Ok(boolean_response(false));
    }
    let drained = match target {
        Target::Host => journal.is_drained().await?,
        Target::Connection {
            connection_id,
            binding,
        } => {
            if connection_id != binding.connection_id || !configured_exactly(state, &binding)? {
                return Ok(boolean_response(false));
            }
            journal
                .connection_scope(&binding)
                .await?
                .is_drained()
                .await?
        }
    };
    Ok(boolean_response(
        drained && read_drain_token(&marker).as_deref() == Some(token.as_str()),
    ))
}

async fn reactivate(
    journal: &Journal,
    state: &ControlState,
    state_path: &Path,
    binding: crate::config::RepositoryConnection,
    token: String,
) -> Result<Response, HostError> {
    let marker = state_path
        .join("connection-drain")
        .join(binding.connection_id.as_str());
    let global_drain = state_path.join("drain");
    if marker_exists(&global_drain)?
        || read_drain_token(&marker).as_deref() != Some(token.as_str())
        || connection_id_present(state, &binding)?
    {
        return Ok(boolean_response(false));
    }
    let reactivated = journal.reactivate_if_drained(&binding).await?;
    Ok(boolean_response(reactivated))
}

async fn detached(
    journal: &Journal,
    state: &ControlState,
    state_path: &Path,
    binding: crate::config::RepositoryConnection,
    token: String,
) -> Result<Response, HostError> {
    let marker = state_path
        .join("connection-drain")
        .join(binding.connection_id.as_str());
    if read_drain_token(&marker).as_deref() != Some(token.as_str())
        || connection_id_present(state, &binding)?
    {
        return Ok(boolean_response(false));
    }
    let detached = journal.is_retired_and_drained(&binding).await?;
    Ok(boolean_response(
        detached && read_drain_token(&marker).as_deref() == Some(token.as_str()),
    ))
}

async fn doctor_probe_start(
    journal: &Journal,
    nonce: &str,
    engine_id: &str,
    docker_root_digest: &str,
) -> Result<Response, HostError> {
    let reply = match journal
        .start_doctor_probe_request(nonce, engine_id, docker_root_digest)
        .await?
    {
        DoctorProbeStartResult::Created(ticket) | DoctorProbeStartResult::Replayed(ticket) => {
            DoctorProbeReply {
                request_id: Some(ticket.request_id()),
                state: "queued".to_owned(),
                report: None,
            }
        }
        DoctorProbeStartResult::Busy { active_request_id } => DoctorProbeReply {
            request_id: Some(active_request_id),
            state: "busy".to_owned(),
            report: None,
        },
    };
    Ok(probe_response(reply))
}

async fn doctor_probe_status(
    journal: &Journal,
    state: &ControlState,
    nonce: &str,
) -> Result<Response, HostError> {
    let reply = match journal.doctor_probe_request_by_nonce(nonce).await? {
        Some(record) => DoctorProbeReply {
            request_id: Some(record.request_id()),
            state: record.state().as_str().to_owned(),
            report: if record.state() == DoctorProbeState::Completed {
                state.doctor_report(nonce)?
            } else {
                None
            },
        },
        None => DoctorProbeReply {
            request_id: None,
            state: "missing".to_owned(),
            report: None,
        },
    };
    Ok(probe_response(reply))
}

fn boolean_response(value: bool) -> Response {
    Response {
        value: Some(value),
        doctor_probe: None,
    }
}

fn probe_response(reply: DoctorProbeReply) -> Response {
    Response {
        value: None,
        doctor_probe: Some(reply),
    }
}
