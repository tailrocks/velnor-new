use super::{EnsureError, EnsuredSet, GITHUB_API, ensured_from_record, observed_scale_set};
use crate::{
    config::ScaleSetSelector,
    error::HostError,
    https::HttpsTransport,
    journal::{
        ConnectionJournal, CreatedScaleSetResponse, ScaleSetCreateNoEffect, ScaleSetCreateRequest,
    },
};
use velnor_runner_github::{
    AdminConnectionCall, CreateLabel, Exchange, RegistrationScope, RegistrationTokenCall,
    RunnerGroup, ScaleSetByName, ScaleSetCreate, ScaleSetFound, ScaleSetView, SessionError,
    SessionRequest, Transport, TransportFail, WireError, admin_connection, create_runner_scale_set,
    get_runner_scale_set, registration_token,
    workflow_run::{
        RepositoryTarget, VerifiedRepositoryScope, WorkflowRunError, get_repository_scope,
    },
};

pub(super) fn open_admin(
    pat: &str,
    scope: &VerifiedRepositoryScope,
) -> Result<(Recording, velnor_runner_github::AdminConnection), EnsureError> {
    let mut transport = Recording::new(HttpsTransport::new(GITHUB_API).map_err(map_host)?);
    let registration = registration_token(
        &mut transport,
        &RegistrationTokenCall {
            scope: RegistrationScope::Repository {
                owner: scope.owner(),
                repo: scope.name(),
            },
            pat,
        },
    )
    .map_err(|err| map_session(err, &transport))?;
    let config_url = format!("https://github.com/{}/{}", scope.owner(), scope.name());
    let admin = admin_connection(
        &mut transport,
        &AdminConnectionCall {
            config_url: &config_url,
            registration_token: registration.expose(),
        },
    )
    .map_err(|err| map_session(err, &transport))?;
    transport
        .inner
        .set_base(admin.expose_url())
        .map_err(map_host)?;
    Ok((transport, admin))
}

pub(super) fn revalidate_repository_scope(
    pat: &str,
    expected: &VerifiedRepositoryScope,
) -> Result<(), EnsureError> {
    let mut transport = HttpsTransport::new(GITHUB_API).map_err(map_host)?;
    let current = get_repository_scope(
        &mut transport,
        RepositoryTarget {
            id: expected.id(),
            owner: expected.owner(),
            name: expected.name(),
        },
        pat,
    )
    .map_err(map_repository)?;
    if current == *expected {
        Ok(())
    } else {
        Err(EnsureError::Rejected)
    }
}

pub(super) fn map_repository(error: WorkflowRunError) -> EnsureError {
    match error {
        WorkflowRunError::InvalidRequest | WorkflowRunError::RepositoryMismatch => {
            EnsureError::Rejected
        }
        WorkflowRunError::Uncertain => EnsureError::Uncertain,
        WorkflowRunError::HttpStatus(403) => EnsureError::Forbidden,
        WorkflowRunError::Malformed | WorkflowRunError::ResponseTooLarge => EnsureError::Malformed,
        WorkflowRunError::HttpStatus(status) => EnsureError::Unexpected {
            status,
            step: "repository-identity",
        },
    }
}

pub(super) fn map_host(error: HostError) -> EnsureError {
    match error {
        HostError::Endpoint => EnsureError::Endpoint,
        HostError::Identity => EnsureError::Rejected,
        HostError::Journal => EnsureError::Unexpected {
            status: 0,
            step: "journal",
        },
        _ => EnsureError::Unexpected {
            status: 0,
            step: "endpoint",
        },
    }
}

pub(super) fn map_session(error: SessionError, transport: &Recording) -> EnsureError {
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

pub(super) struct Recording {
    pub(super) inner: HttpsTransport,
    pub(super) status: u16,
    step: &'static str,
}

impl Recording {
    const fn new(inner: HttpsTransport) -> Self {
        Self {
            inner,
            status: 0,
            step: "registration-token",
        }
    }
}

impl Transport for Recording {
    fn exchange(&mut self, request: &SessionRequest) -> Result<Exchange, TransportFail> {
        self.step = step_for(&request.path);
        let exchange = self.inner.exchange(request)?;
        self.status = exchange.status;
        Ok(exchange)
    }
}

fn step_for(path: &str) -> &'static str {
    if path.contains("registration-token") {
        "registration-token"
    } else if path.contains("runner-registration") {
        "runner-registration"
    } else if path.contains("acquirejobs") {
        "acquire"
    } else if path.contains("generatejitconfig") {
        "jit"
    } else if path.contains("runnergroups") {
        "runnergroups"
    } else {
        "runnerscalesets"
    }
}

pub(super) async fn create_connection_scale_set(
    pat: &str,
    scope: &VerifiedRepositoryScope,
    transport: &mut Recording,
    admin_token: &str,
    scoped: &ConnectionJournal,
    request: &ScaleSetCreateRequest,
    record: &crate::journal::ScaleSetCreateRecord,
) -> Result<EnsuredSet, EnsureError> {
    revalidate_repository_scope(pat, scope)?;
    let Some(claim) = scoped
        .claim_scale_set_create(record)
        .await
        .map_err(map_host)?
    else {
        return Err(EnsureError::Uncertain);
    };
    let labels = create_labels(request.selector_labels());
    let view = match create_runner_scale_set(
        transport,
        &ScaleSetCreate {
            name: request.selector_name(),
            runner_group_id: request.runner_group_id(),
            labels: &labels,
            admin_token,
        },
    ) {
        Ok(view) => view,
        Err(_error) if transport.status == 409 => {
            let no_effect = ScaleSetCreateNoEffect::from_http_status(409).map_err(map_host)?;
            scoped
                .record_scale_set_create_no_effect(claim, no_effect)
                .await
                .map_err(map_host)?;
            return adopt_scale_set_after_conflict(
                pat,
                scope,
                transport,
                admin_token,
                scoped,
                request,
            )
            .await;
        }
        Err(error) => {
            scoped
                .mark_scale_set_create_uncertain(claim)
                .await
                .map_err(map_host)?;
            return Err(map_session(error, transport));
        }
    };
    let label_names = scale_set_label_names(&view);
    let response = match CreatedScaleSetResponse::verified(
        request,
        view.id,
        &view.name,
        &label_names,
        view.runner_setting.disable_update,
    ) {
        Ok(response) => response,
        Err(error) => {
            scoped
                .mark_scale_set_create_uncertain(claim)
                .await
                .map_err(map_host)?;
            return Err(map_host(error));
        }
    };
    let created = scoped
        .record_scale_set_created(claim, response)
        .await
        .map_err(map_host)?;
    ensured_from_record(&created)
}

async fn adopt_scale_set_after_conflict(
    pat: &str,
    scope: &VerifiedRepositoryScope,
    transport: &mut Recording,
    admin_token: &str,
    scoped: &ConnectionJournal,
    request: &ScaleSetCreateRequest,
) -> Result<EnsuredSet, EnsureError> {
    let found = get_runner_scale_set(
        transport,
        &ScaleSetByName {
            runner_group_id: request.runner_group_id(),
            name: request.selector_name(),
            expected_labels: request.selector_labels(),
            admin_token,
        },
    )
    .map_err(|error| map_session(error, transport))?;
    let ScaleSetFound::Found(view) = found else {
        return Err(EnsureError::Uncertain);
    };
    revalidate_repository_scope(pat, scope)?;
    let observed = observed_scale_set(request, view)?;
    let adopted = scoped
        .adopt_scale_set(request, observed)
        .await
        .map_err(map_host)?;
    ensured_from_record(&adopted)
}

fn scale_set_label_names(view: &ScaleSetView) -> Vec<String> {
    view.labels.iter().map(|label| label.name.clone()).collect()
}

pub(super) fn default_group_id(groups: &[RunnerGroup]) -> Result<i64, EnsureError> {
    let mut defaults = groups.iter().filter(|group| group.is_default);
    let Some(group) = defaults.next() else {
        return Err(EnsureError::Rejected);
    };
    if group.id <= 0 || defaults.next().is_some() {
        return Err(EnsureError::Rejected);
    }
    Ok(group.id)
}

pub(super) fn selector_valid(selector: &ScaleSetSelector) -> bool {
    let labels = selector.labels();
    !selector.name().is_empty()
        && labels.len() >= 2
        && labels.first().is_some_and(|label| label == "velnor")
        && labels.get(1).is_some_and(|label| label == selector.name())
        && !labels.iter().any(|label| {
            label.is_empty() || label == "ubuntu-26.04" || label.chars().any(char::is_control)
        })
        && labels
            .iter()
            .enumerate()
            .all(|(index, label)| !labels[..index].iter().any(|prior| prior == label))
}

pub(super) fn create_labels(labels: &[String]) -> Vec<CreateLabel> {
    labels
        .iter()
        .map(|name| CreateLabel {
            name: name.clone(),
            label_type: "System".to_owned(),
        })
        .collect()
}
