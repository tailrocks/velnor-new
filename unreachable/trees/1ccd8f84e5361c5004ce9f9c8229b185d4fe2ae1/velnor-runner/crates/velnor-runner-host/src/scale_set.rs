//! Get or create `ubuntu-26.04-scale-set` with the shipped registration client.

use velnor_runner_github::{
    AdminConnectionCall, Exchange, RegistrationScope, RegistrationTokenCall, RunnerGroup,
    ScaleSetByName, ScaleSetCreate, ScaleSetFound, SessionError, SessionRequest, Transport,
    TransportFail, WireError, admin_connection, create_runner_scale_set, get_runner_scale_set,
    list_runner_groups, product_create_labels, registration_token,
};

use crate::error::HostError;
use crate::https::HttpsTransport;

const SET_NAME: &str = "ubuntu-26.04-scale-set";
const GITHUB_API: &str = "https://api.github.com";
const DEFAULT_GROUP: i64 = 1;

/// Identifiers safe to print. No token and no service URL.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnsuredSet {
    /// Positive scale-set id.
    pub id: i64,
    /// Scale-set name.
    pub name: String,
    /// Registration invariant.
    pub disable_update: bool,
    /// Label names in service order.
    pub labels: Vec<String>,
}

/// Failure that does not include a credential or a response body.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum EnsureError {
    /// The endpoint was not `https`.
    #[error("bad endpoint")]
    Endpoint,
    /// The service rejected the registration invariant.
    #[error("registration rejected")]
    Rejected,
    /// HTTP 403.
    #[error("forbidden")]
    Forbidden,
    /// The body did not match the pinned DTO.
    #[error("malformed message")]
    Malformed,
    /// Timeout or reset. The create may have happened.
    #[error("effect uncertain")]
    Uncertain,
    /// Another session holds the set.
    #[error("session conflict")]
    Conflict,
    /// A finished status was not the one the call expects.
    #[error("unexpected status {status} at {step}")]
    Unexpected {
        /// HTTP status. Zero means the transport failed before a status.
        status: u16,
        /// Which registration call failed.
        step: &'static str,
    },
}

/// Mint an admin connection, then get or create the product scale set.
///
/// `pat` is the caller credential. It is not written and not returned.
/// Runner group `1` is the repository default. A missing set is created with
/// `disableUpdate` and the product labels.
///
/// # Errors
///
/// Returns [`EnsureError`] when the endpoint, token, or scale set is refused.
/// An empty `pat`, owner, or repo fails before any transport call.
pub fn ensure_product_scale_set(
    pat: &str,
    owner: &str,
    repo: &str,
) -> Result<EnsuredSet, EnsureError> {
    if pat.is_empty() || owner.is_empty() || repo.is_empty() {
        return Err(EnsureError::Rejected);
    }
    let (mut transport, admin) = open_admin(pat, owner, repo)?;
    let found = get_runner_scale_set(
        &mut transport,
        &ScaleSetByName {
            runner_group_id: DEFAULT_GROUP,
            name: SET_NAME,
            admin_token: admin.expose_token(),
        },
    )
    .map_err(|err| map_session(err, &transport))?;
    let view = match found {
        ScaleSetFound::Found(view) => view,
        ScaleSetFound::NotFound => create_runner_scale_set(
            &mut transport,
            &ScaleSetCreate {
                name: SET_NAME,
                runner_group_id: DEFAULT_GROUP,
                labels: &product_create_labels(),
                admin_token: admin.expose_token(),
            },
        )
        .map_err(|err| map_session(err, &transport))?,
    };
    Ok(EnsuredSet {
        id: view.id,
        name: view.name,
        disable_update: view.runner_setting.disable_update,
        labels: view.labels.into_iter().map(|label| label.name).collect(),
    })
}

/// Runner groups visible to this repository's Actions connection.
///
/// # Errors
///
/// Returns [`EnsureError`] when the endpoint, token, or group list is refused.
pub fn product_runner_groups(
    pat: &str,
    owner: &str,
    repo: &str,
) -> Result<Vec<RunnerGroup>, EnsureError> {
    if pat.is_empty() || owner.is_empty() || repo.is_empty() {
        return Err(EnsureError::Rejected);
    }
    let (mut transport, admin) = open_admin(pat, owner, repo)?;
    list_runner_groups(&mut transport, admin.expose_token())
        .map_err(|err| map_session(err, &transport))
}

fn open_admin(
    pat: &str,
    owner: &str,
    repo: &str,
) -> Result<(Recording, velnor_runner_github::AdminConnection), EnsureError> {
    let mut transport = Recording::new(HttpsTransport::new(GITHUB_API).map_err(map_host)?);
    let registration = registration_token(
        &mut transport,
        &RegistrationTokenCall {
            scope: RegistrationScope::Repository { owner, repo },
            pat,
        },
    )
    .map_err(|err| map_session(err, &transport))?;
    let config_url = format!("https://github.com/{owner}/{repo}");
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

fn map_host(error: HostError) -> EnsureError {
    match error {
        HostError::Endpoint => EnsureError::Endpoint,
        _ => EnsureError::Unexpected {
            status: 0,
            step: "endpoint",
        },
    }
}

fn map_session(error: SessionError, transport: &Recording) -> EnsureError {
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

struct Recording {
    inner: HttpsTransport,
    status: u16,
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
