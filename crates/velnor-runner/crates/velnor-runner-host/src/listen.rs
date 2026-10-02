//! One session probe. Deletes the session before returning. No JIT and no job acquire.

use velnor_runner_github::{
    AdminConnection, AdminConnectionCall, Poll, QueueSession, RefreshGate, RegistrationScope,
    RegistrationTokenCall, SessionError, WireError, admin_connection, create_session,
    delete_session, poll, registration_token,
};
use zeroize::Zeroize;

use crate::Offer;
use crate::offer;

use crate::error::HostError;
use crate::https::HttpsTransport;
use crate::scale_set::EnsureError;
use crate::scale_set::ensure_product_scale_set;

const OWNER_NAME: &str = "velnor-host";
const GITHUB_API: &str = "https://api.github.com";

/// What one short session saw. No token and no queue URL.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SessionProbe {
    /// Scale-set id.
    pub set_id: i64,
    /// A `JobAvailable` id was waiting. It was not acquired.
    pub available: bool,
}

/// Open one session, poll once, then delete that session.
///
/// # Errors
///
/// Returns [`EnsureError`] when registration, the session, or the delete is refused.
/// The session delete still runs when the poll fails.
pub fn probe_once(pat: &str, owner: &str, repo: &str) -> Result<SessionProbe, EnsureError> {
    let set = ensure_product_scale_set(pat, owner, repo)?;
    let mut link = admin_link(pat, owner, repo)?;
    let token = Secret::new(link.token());
    let session = create_session(&mut link.transport, set.id, OWNER_NAME, token.expose())
        .map_err(|err| annotate(err, "create-session"))?;
    let polled = poll_available(&mut link, &session);
    let closed = delete_session(
        &mut link.transport,
        set.id,
        &session.session_id,
        token.expose(),
    );
    let available = polled?;
    closed.map_err(map_listen)?;
    Ok(SessionProbe {
        set_id: set.id,
        available,
    })
}

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

struct Secret(String);

impl Secret {
    fn new(text: &str) -> Self {
        Self(text.to_owned())
    }

    fn expose(&self) -> &str {
        &self.0
    }
}

impl Drop for Secret {
    fn drop(&mut self) {
        self.0.zeroize();
    }
}

struct Link {
    transport: HttpsTransport,
    admin: AdminConnection,
    base: String,
}

impl Link {
    fn token(&self) -> &str {
        self.admin.expose_token()
    }
}

fn admin_link(pat: &str, owner: &str, repo: &str) -> Result<Link, EnsureError> {
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

fn poll_available(link: &mut Link, session: &QueueSession) -> Result<bool, EnsureError> {
    let (saved, path) = point_at_queue(link, &session.message_queue_url)?;
    let polled = poll_path(link, session, &path);
    restore_base(link, saved)?;
    let polled = polled?;
    Ok(matches!(offer(&polled), Offer::Acquire { .. }))
}

fn point_at_queue(link: &mut Link, url: &str) -> Result<(Option<String>, String), EnsureError> {
    if let Some(path) = queue_path(&link.base, url) {
        return Ok((None, path.to_owned()));
    }
    let absolute = absolute_https(url).ok_or(EnsureError::Unexpected {
        status: 0,
        step: "queue-path",
    })?;
    let saved = link.base.clone();
    link.transport
        .set_base(&absolute.origin)
        .map_err(map_host)?;
    link.base = absolute.origin;
    Ok((Some(saved), absolute.path))
}

fn poll_path(link: &mut Link, session: &QueueSession, path: &str) -> Result<Poll, EnsureError> {
    let gate = RefreshGate::new();
    let refresh = || Ok::<(), WireError>(());
    poll(
        &mut link.transport,
        path,
        0,
        1,
        session.token(),
        &gate,
        refresh,
    )
    .map_err(|err| annotate(err, "poll"))
}

fn restore_base(link: &mut Link, saved: Option<String>) -> Result<(), EnsureError> {
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

fn map_host(error: HostError) -> EnsureError {
    match error {
        HostError::Endpoint => EnsureError::Endpoint,
        _ => EnsureError::Unexpected {
            status: 0,
            step: "endpoint",
        },
    }
}

fn annotate(error: SessionError, step: &'static str) -> EnsureError {
    match error {
        SessionError::Wire(WireError::Malformed) => EnsureError::Unexpected { status: 0, step },
        other => map_listen(other),
    }
}

fn map_listen(error: SessionError) -> EnsureError {
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
