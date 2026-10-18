//! One session probe. Deletes the session before returning. No JIT and no job acquire.

use velnor_runner_github::{
    AdminConnection, AdminConnectionCall, Poll, QueueSession, RefreshGate, RegistrationScope,
    RegistrationTokenCall, SessionError, WireError, admin_connection, poll, registration_token,
    workflow_run::VerifiedRepositoryScope,
};
use zeroize::Zeroize;

use crate::config::RepositoryConnection;
use crate::error::HostError;
use crate::https::HttpsTransport;
use crate::scale_set::EnsureError;

/// Session owner sent to the scale-set service.
pub(crate) const OWNER_NAME: &str = "velnor-host";
pub(crate) const GITHUB_API: &str = "https://api.github.com";

/// Path relative to the admin origin. Absolute URLs that are not under `base` are rejected.
#[must_use]
pub fn queue_path<'a>(base: &str, queue_url: &'a str) -> Option<&'a str> {
    let relative = queue_url
        .strip_prefix(base)
        .unwrap_or(queue_url)
        .trim_start_matches('/');
    if relative.is_empty() || relative.contains("://") {
        None
    } else {
        Some(relative)
    }
}

/// Admin bearer copy. Zeroized on drop.
pub(crate) struct Secret(String);

impl Secret {
    /// Copy `text`. The copy is zeroized on drop.
    #[must_use]
    pub(crate) fn new(text: &str) -> Self {
        Self(text.to_owned())
    }

    /// Borrow the bearer for one header. Not for logs.
    #[must_use]
    pub(crate) fn expose(&self) -> &str {
        &self.0
    }
}

impl Drop for Secret {
    fn drop(&mut self) {
        self.0.zeroize();
    }
}

/// Admin client. The base moves between the service and the message host.
pub(crate) struct Link {
    transport: HttpsTransport,
    admin: AdminConnection,
    base: String,
}

/// Continue with the exact admin connection used to register a Scale Set.
///
/// The caller must have authenticated the repository scope before obtaining
/// `admin`; this function does not create or broaden credentials.
pub(crate) fn link_from_admin(
    mut transport: HttpsTransport,
    admin: AdminConnection,
    binding: &RepositoryConnection,
    verified_scope: &VerifiedRepositoryScope,
) -> Result<Link, EnsureError> {
    let (owner, name) = binding
        .repository
        .split_once('/')
        .ok_or(EnsureError::Rejected)?;
    if owner.is_empty()
        || name.is_empty()
        || verified_scope.id() != binding.repository_id
        || verified_scope.owner() != owner
        || verified_scope.name() != name
    {
        return Err(EnsureError::Rejected);
    }
    let base = admin.expose_url().to_owned();
    transport.set_base(&base).map_err(map_host)?;
    Ok(Link {
        transport,
        admin,
        base,
    })
}

impl Link {
    /// Admin bearer. Not for logs.
    #[must_use]
    pub(crate) fn token(&self) -> &str {
        self.admin.expose_token()
    }

    /// Current service origin.
    #[must_use]
    pub(crate) fn base(&self) -> &str {
        &self.base
    }

    /// Session transport. Do not hold a journal connection across this.
    pub(crate) fn transport(&mut self) -> &mut HttpsTransport {
        &mut self.transport
    }

    /// Point this client at `base`.
    ///
    /// # Errors
    ///
    /// Returns [`EnsureError::Endpoint`] when `base` is not `https`.
    pub(crate) fn set_base(&mut self, base: &str) -> Result<(), EnsureError> {
        self.transport.set_base(base).map_err(map_host)?;
        base.clone_into(&mut self.base);
        Ok(())
    }
}

/// Registration plus the admin service origin.
///
/// # Errors
///
/// Returns [`EnsureError`] when the token or the admin connection is refused.
pub(crate) fn admin_link(pat: &str, owner: &str, repo: &str) -> Result<Link, EnsureError> {
    let mut transport = HttpsTransport::new(GITHUB_API).map_err(map_host)?;
    let registration = registration_token(
        &mut transport,
        &RegistrationTokenCall {
            scope: RegistrationScope::Repository { owner, repo },
            pat,
        },
    )
    .map_err(map_listen)?;
    let config_url = format!("https://github.com/{owner}/{repo}");
    let admin = admin_connection(
        &mut transport,
        &AdminConnectionCall {
            config_url: &config_url,
            registration_token: registration.expose(),
        },
    )
    .map_err(map_listen)?;
    let base = admin.expose_url().to_owned();
    transport.set_base(&base).map_err(map_host)?;
    Ok(Link {
        transport,
        admin,
        base,
    })
}

/// Move onto an absolute queue host. `None` means the queue is already on this origin.
///
/// # Errors
///
/// Returns [`EnsureError`] when the URL is not a usable `https` queue.
pub(crate) fn point_at_queue(
    link: &mut Link,
    url: &str,
) -> Result<(Option<String>, String), EnsureError> {
    let (origin, path) = resolve_queue_route(&link.base, url)?;
    let Some(origin) = origin else {
        return Ok((None, path));
    };
    let saved = link.base.clone();
    link.transport.set_base(&origin).map_err(map_host)?;
    link.base = origin;
    Ok((Some(saved), path))
}

/// Check a refreshed queue URL with the same rules used before each queue request.
pub(crate) fn validate_queue_url(base: &str, url: &str) -> Result<(), EnsureError> {
    resolve_queue_route(base, url).map(|_| ())
}

fn resolve_queue_route(base: &str, url: &str) -> Result<(Option<String>, String), EnsureError> {
    if let Some(path) = queue_path(base, url) {
        return Ok((None, path.to_owned()));
    }
    let absolute = absolute_https(url).ok_or(EnsureError::Unexpected {
        status: 0,
        step: "queue-path",
    })?;
    Ok((Some(absolute.origin), absolute.path))
}

/// One poll on the current origin.
///
/// `total_capacity` is the `X-ScaleSetMaxCapacity` header, not free slots.
/// `cursor` is `lastMessageId`. Zero reads the queue head.
///
/// # Errors
///
/// Returns [`EnsureError`] when the poll is refused.
pub(crate) fn poll_path(
    link: &mut Link,
    session: &QueueSession,
    path: &str,
    total_capacity: u32,
    cursor: i64,
) -> Result<Poll, EnsureError> {
    let gate = RefreshGate::new();
    let refresh = || Ok::<(), WireError>(());
    poll(
        &mut link.transport,
        path,
        cursor,
        total_capacity,
        session.token(),
        &gate,
        refresh,
    )
    .map_err(|err| annotate(err, "poll"))
}

/// Restore the admin origin saved by [`point_at_queue`].
///
/// # Errors
///
/// Returns [`EnsureError::Endpoint`] when the saved origin is not `https`.
pub(crate) fn restore_base(link: &mut Link, saved: Option<String>) -> Result<(), EnsureError> {
    let Some(base) = saved else {
        return Ok(());
    };
    link.transport.set_base(&base).map_err(map_host)?;
    link.base = base;
    Ok(())
}

pub(crate) struct Absolute {
    pub(crate) origin: String,
    pub(crate) path: String,
}

pub(crate) fn absolute_https(url: &str) -> Option<Absolute> {
    let rest = url.strip_prefix("https://")?;
    let (host, path) = rest.split_once('/')?;
    if host.is_empty() || path.is_empty() || host.contains('@') {
        return None;
    }
    Some(Absolute {
        origin: format!("https://{host}"),
        path: path.to_owned(),
    })
}

/// Map a local endpoint failure. Other host errors stay off the body.
pub(crate) fn map_host(error: HostError) -> EnsureError {
    match error {
        HostError::Endpoint => EnsureError::Endpoint,
        _ => EnsureError::Unexpected {
            status: 0,
            step: "endpoint",
        },
    }
}

/// Attach `step` when the body was malformed. Other failures stay as [`map_listen`].
#[must_use]
pub(crate) fn annotate(error: SessionError, step: &'static str) -> EnsureError {
    match error {
        SessionError::Wire(WireError::Malformed) => EnsureError::Unexpected { status: 0, step },
        other => map_listen(other),
    }
}

/// Map a session failure without copying a response body.
#[must_use]
pub(crate) fn map_listen(error: SessionError) -> EnsureError {
    match error {
        SessionError::Uncertain => EnsureError::Uncertain,
        SessionError::Conflict => EnsureError::Conflict,
        SessionError::Wire(WireError::RegistrationRejected) => EnsureError::Rejected,
        SessionError::Wire(WireError::Forbidden) => EnsureError::Forbidden,
        SessionError::Wire(WireError::Malformed) => EnsureError::Malformed,
        SessionError::Wire(_) => EnsureError::Unexpected {
            status: 0,
            step: "session",
        },
    }
}
