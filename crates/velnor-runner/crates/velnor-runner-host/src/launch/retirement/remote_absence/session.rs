use velnor_runner_github::{
    AdminConnection, AdminConnectionCall, RegistrationScope, RegistrationTokenCall,
    RunnerAbsenceReceipt, RunnerReference, ScaleSetByName, ScaleSetFound, SessionError,
    SessionRequest, Transport, TransportFail, WireError, admin_connection, ensure_runner_absent,
    get_runner_scale_set, registration_token,
};

use crate::{
    config::RepositoryConnection, https::HttpsTransport, journal::PreparedRetirementRemoteAbsence,
    scale_set::EnsureError,
};

use super::repository::RepositoryParts;

#[cfg(test)]
#[path = "session_tests.rs"]
mod tests;

pub(super) struct Recording<T = HttpsTransport> {
    inner: T,
    status: u16,
    step: &'static str,
}

impl<T> Recording<T> {
    pub(super) const fn new(inner: T) -> Self {
        Self {
            inner,
            status: 0,
            step: "registration-token",
        }
    }
}

pub(super) trait AdminEndpointTransport: Transport {
    fn set_base(&mut self, base: &str) -> Result<(), EnsureError>;
}

impl AdminEndpointTransport for HttpsTransport {
    fn set_base(&mut self, base: &str) -> Result<(), EnsureError> {
        HttpsTransport::set_base(self, base).map_err(|_| EnsureError::Endpoint)
    }
}

impl<T: AdminEndpointTransport> Recording<T> {
    pub(super) fn set_base(&mut self, base: &str) -> Result<(), EnsureError> {
        self.inner.set_base(base)
    }
}

impl<T: Transport> Transport for Recording<T> {
    fn exchange(
        &mut self,
        request: &SessionRequest,
    ) -> Result<velnor_runner_github::Exchange, TransportFail> {
        self.step = step_for(&request.path);
        let response = self.inner.exchange(request)?;
        self.status = response.status;
        Ok(response)
    }
}

pub(super) fn register_repository<T: Transport>(
    transport: &mut Recording<T>,
    parts: &RepositoryParts<'_>,
    pat: &str,
) -> Result<AdminConnection, EnsureError> {
    let token = registration_token(
        transport,
        &RegistrationTokenCall {
            scope: RegistrationScope::Repository {
                owner: parts.owner,
                repo: parts.name,
            },
            pat,
        },
    )
    .map_err(|error| map_session(error, transport))?;
    let config_url = format!("https://github.com/{}/{}", parts.owner, parts.name);
    admin_connection(
        transport,
        &AdminConnectionCall {
            config_url: &config_url,
            registration_token: token.expose(),
        },
    )
    .map_err(|error| map_session(error, transport))
}

pub(super) fn confirm_runner_absence<T: Transport>(
    transport: &mut Recording<T>,
    binding: &RepositoryConnection,
    expected: &PreparedRetirementRemoteAbsence,
    admin: &AdminConnection,
) -> Result<RunnerAbsenceReceipt, EnsureError> {
    let labels = binding.scale_set.labels();
    let set = get_runner_scale_set(
        transport,
        &ScaleSetByName {
            runner_group_id: expected.runner_group_id(),
            name: binding.scale_set.name(),
            expected_labels: labels,
            admin_token: admin.expose_token(),
        },
    )
    .map_err(|error| map_session(error, transport))?;
    match set {
        ScaleSetFound::Found(view)
            if view.id == expected.scale_set_id()
                && view.name == binding.scale_set.name()
                && view.runner_setting.disable_update => {}
        ScaleSetFound::Found(_) | ScaleSetFound::NotFound => return Err(EnsureError::Rejected),
    }
    let runner = RunnerReference {
        id: expected.runner_id(),
        name: expected.runner_name().to_owned(),
        runner_scale_set_id: expected.scale_set_id(),
    };
    ensure_runner_absent(transport, &runner, admin.expose_token())
        .map_err(|error| map_session(error, transport))
}

fn map_session<T>(error: SessionError, transport: &Recording<T>) -> EnsureError {
    match error {
        SessionError::Uncertain => EnsureError::Uncertain,
        SessionError::Conflict => EnsureError::Conflict,
        SessionError::Wire(WireError::RegistrationRejected) => EnsureError::Rejected,
        SessionError::Wire(WireError::Forbidden) => EnsureError::Forbidden,
        SessionError::Wire(WireError::Malformed) => EnsureError::Malformed,
        SessionError::Wire(_) => EnsureError::Unexpected {
            status: transport.status,
            step: transport.step,
        },
    }
}

fn step_for(path: &str) -> &'static str {
    if path.contains("registration-token") {
        "registration-token"
    } else if path.contains("runner-registration") {
        "runner-registration"
    } else if path.contains("runnerscalesets") {
        "scale-set-identity"
    } else if path.contains("/runners/") {
        "runner-absence"
    } else {
        "repository-identity"
    }
}
