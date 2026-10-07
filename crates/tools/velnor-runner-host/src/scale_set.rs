//! Resolve the configured runner group and Scale Set before launch.

use velnor_runner_github::{
    AdminConnectionCall, Exchange, RegistrationScope, RegistrationTokenCall, RunnerGroup,
    ScaleSetByName, ScaleSetCreate, ScaleSetFound, SessionError, SessionRequest, Transport,
    TransportFail, WireError, admin_connection, create_runner_scale_set, get_runner_scale_set,
    list_runner_groups, product_create_labels_for, registration_token,
};

use crate::HostError;
use crate::config::{RegistrationScopeKind, ScaleSetBinding};
use crate::https::HttpsTransport;

const GITHUB_API: &str = "https://api.github.com";

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
    /// The exact configured Scale Set is not present.
    #[error("scale set not found")]
    NotFound,
    /// Repository-scoped runner group privacy/workflow policy cannot be proven.
    #[error("runner group policy is unavailable for repository scope")]
    GroupPolicyUnavailable,
    /// A finished status was not the one the call expects.
    #[error("unexpected status {status} at {step}")]
    Unexpected {
        /// HTTP status. Zero means the transport failed before a status.
        status: u16,
        /// Which registration call failed.
        step: &'static str,
    },
}

/// Preserve the existing macOS repository binding: group 1 and the Ubuntu 26.04 selector.
///
/// New Linux callers must use [`ensure_product_scale_set_for_binding`] with the
/// explicit validated scope, group, selector, and image profile.
///
/// # Errors
///
/// Returns [`EnsureError`] when registration or the legacy set is refused.
pub fn ensure_product_scale_set(
    pat: &str,
    owner: &str,
    repo: &str,
) -> Result<EnsuredSet, EnsureError> {
    let binding = ScaleSetBinding {
        scope: RegistrationScopeKind::Repository,
        owner: owner.to_owned(),
        repository: repo.to_owned(),
        scale_set_name: "ubuntu-26.04-scale-set".to_owned(),
        runner_group_id: 1,
        runner_group_name: "Default".to_owned(),
        runner_image_profile: None,
    };
    ensure_product_scale_set_for_binding(pat, &binding)
}

/// Read and validate the configured Scale Set, creating it only when absent.
///
/// `pat` is the caller credential. It is not written and not returned. The
/// exact repository registration scope, runner-group id/name, selector, and
/// image profile come from the validated host configuration.
///
/// # Errors
///
/// Returns [`EnsureError`] when the endpoint, token, or scale set is refused.
/// An empty `pat`, owner, or repo fails before any transport call.
pub fn ensure_product_scale_set_for_binding(
    pat: &str,
    binding: &ScaleSetBinding,
) -> Result<EnsuredSet, EnsureError> {
    validate_binding(pat, binding)?;
    require_group_policy_evidence(binding)?;
    let (mut transport, admin) = open_admin(pat, &binding.owner, &binding.repository)?;
    validate_runner_group(&mut transport, admin.expose_token(), binding)?;
    let found = get_runner_scale_set(
        &mut transport,
        &ScaleSetByName {
            runner_group_id: binding.runner_group_id,
            name: &binding.scale_set_name,
            admin_token: admin.expose_token(),
        },
    )
    .map_err(|err| map_session(err, &transport))?;
    let view = match found {
        ScaleSetFound::Found(view) => view,
        ScaleSetFound::NotFound => {
            let labels = product_create_labels_for(&binding.scale_set_name).map_err(map_wire)?;
            create_runner_scale_set(
                &mut transport,
                &ScaleSetCreate {
                    name: &binding.scale_set_name,
                    runner_group_id: binding.runner_group_id,
                    labels: &labels,
                    admin_token: admin.expose_token(),
                },
            )
            .map_err(|err| map_session(err, &transport))?
        }
    };
    Ok(ensured_set(view))
}

/// Resolve the configured group and existing Scale Set during `connect`.
///
/// This exchange mints short-lived registration and admin credentials before
/// the exact group and set lookup. It never creates a Scale Set. Diagnostics
/// must use the local read-only lifecycle path instead of calling this API.
///
/// # Errors
///
/// Returns [`EnsureError::NotFound`] when the exact set is absent.
pub fn discover_product_scale_set(
    pat: &str,
    binding: &ScaleSetBinding,
) -> Result<EnsuredSet, EnsureError> {
    validate_binding(pat, binding)?;
    require_group_policy_evidence(binding)?;
    let (mut transport, admin) = open_admin(pat, &binding.owner, &binding.repository)?;
    validate_runner_group(&mut transport, admin.expose_token(), binding)?;
    let found = get_runner_scale_set(
        &mut transport,
        &ScaleSetByName {
            runner_group_id: binding.runner_group_id,
            name: &binding.scale_set_name,
            admin_token: admin.expose_token(),
        },
    )
    .map_err(|err| map_session(err, &transport))?;
    match found {
        ScaleSetFound::Found(view) => Ok(ensured_set(view)),
        ScaleSetFound::NotFound => Err(EnsureError::NotFound),
    }
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

fn validate_binding(pat: &str, binding: &ScaleSetBinding) -> Result<(), EnsureError> {
    if pat.is_empty()
        || binding.scope != RegistrationScopeKind::Repository
        || !safe_segment(&binding.owner)
        || !safe_segment(&binding.repository)
        || !safe_scale_set_name(&binding.scale_set_name)
        || binding.runner_group_id <= 0
        || binding.runner_group_name.trim().is_empty()
    {
        return Err(EnsureError::Rejected);
    }
    match binding.runner_image_profile.as_deref() {
        Some(profile) => {
            let _profile =
                crate::docker_spec::resolve_runner_profile(profile, &binding.scale_set_name)
                    .map_err(|_| EnsureError::Rejected)?;
        }
        None if binding.scale_set_name == "ubuntu-26.04-scale-set" => {}
        None => return Err(EnsureError::Rejected),
    }
    validate_group_identity(binding)
}

/// Stop before token exchange or Scale Set effects when no supported policy
/// read exists for the repository-scoped group. The current protocol DTO only
/// provides group identity; it cannot prove private repository/workflow scope.
fn require_group_policy_evidence(binding: &ScaleSetBinding) -> Result<(), EnsureError> {
    if binding.runner_image_profile.is_some() {
        Err(EnsureError::GroupPolicyUnavailable)
    } else {
        Ok(())
    }
}

fn validate_runner_group(
    transport: &mut Recording,
    admin_token: &str,
    binding: &ScaleSetBinding,
) -> Result<(), EnsureError> {
    let groups = list_runner_groups(transport, admin_token)
        .map_err(|error| map_session(error, transport))?;
    validate_group(&groups, binding)
}

fn validate_group_identity(binding: &ScaleSetBinding) -> Result<(), EnsureError> {
    if binding.runner_group_name.trim().is_empty()
        || binding.runner_group_name.chars().any(char::is_control)
    {
        Err(EnsureError::Rejected)
    } else {
        Ok(())
    }
}

fn validate_group(groups: &[RunnerGroup], binding: &ScaleSetBinding) -> Result<(), EnsureError> {
    let id_matches = groups
        .iter()
        .filter(|group| group.id == binding.runner_group_id)
        .count();
    let name_matches = groups
        .iter()
        .filter(|group| group.name == binding.runner_group_name)
        .count();
    if id_matches != 1
        || name_matches != 1
        || !groups.iter().any(|group| {
            group.id == binding.runner_group_id && group.name == binding.runner_group_name
        })
    {
        return Err(EnsureError::Rejected);
    }
    Ok(())
}

fn ensured_set(view: velnor_runner_github::ScaleSetView) -> EnsuredSet {
    EnsuredSet {
        id: view.id,
        name: view.name,
        disable_update: view.runner_setting.disable_update,
        labels: view.labels.into_iter().map(|label| label.name).collect(),
    }
}

fn map_wire(error: WireError) -> EnsureError {
    match error {
        WireError::RegistrationRejected => EnsureError::Rejected,
        _ => EnsureError::Malformed,
    }
}

fn safe_segment(value: &str) -> bool {
    !value.is_empty()
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
}

fn safe_scale_set_name(value: &str) -> bool {
    safe_segment(value)
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

#[cfg(test)]
mod tests;
