// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

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

pub(crate) const OWNER_NAME: &str = "velnor-host";
pub(crate) const GITHUB_API: &str = "https://api.github.com";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SessionCensus {
    pub set_id: i64,

    pub statistics_present: bool,

    pub available_jobs: i64,

    pub acquired_jobs: i64,

    pub assigned_jobs: i64,

    pub running_jobs: i64,

    pub registered_runners: i64,

    pub busy_runners: i64,

    pub idle_runners: i64,
}

pub fn session_census(pat: &str, owner: &str, repo: &str) -> Result<SessionCensus, EnsureError> {
    let set = ensure_product_scale_set(pat, owner, repo)?;
    let mut link = admin_link(pat, owner, repo)?;
    let token = Secret::new(link.token());
    let session = create_session(&mut link.transport, set.id, OWNER_NAME, token.expose())
        .map_err(|err| annotate(err, "create-session"))?;
    let census = census_of(set.id, &session);
    delete_session(
        &mut link.transport,
        set.id,
        &session.session_id,
        token.expose(),
    )
    .map_err(map_listen)?;
    Ok(census)
}

fn census_of(set_id: i64, session: &QueueSession) -> SessionCensus {
    let Some(stats) = session.statistics() else {
        return SessionCensus {
            set_id,
            statistics_present: false,
            available_jobs: 0,
            acquired_jobs: 0,
            assigned_jobs: 0,
            running_jobs: 0,
            registered_runners: 0,
            busy_runners: 0,
            idle_runners: 0,
        };
    };
    SessionCensus {
        set_id,
        statistics_present: true,
        available_jobs: stats.total_available_jobs,
        acquired_jobs: stats.total_acquired_jobs,
        assigned_jobs: stats.total_assigned_jobs,
        running_jobs: stats.total_running_jobs,
        registered_runners: stats.total_registered_runners,
        busy_runners: stats.total_busy_runners,
        idle_runners: stats.total_idle_runners,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SessionProbe {
    pub set_id: i64,

    pub available: bool,
}

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

pub(crate) struct Secret(String);

impl Secret {
    #[must_use]
    pub(crate) fn new(text: &str) -> Self {
        Self(text.to_owned())
    }

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

pub(crate) struct Link {
    transport: HttpsTransport,
    admin: AdminConnection,
    base: String,
}

impl Link {
    #[must_use]
    pub(crate) fn token(&self) -> &str {
        self.admin.expose_token()
    }

    #[must_use]
    pub(crate) fn base(&self) -> &str {
        &self.base
    }

    pub(crate) fn transport(&mut self) -> &mut HttpsTransport {
        &mut self.transport
    }

    pub(crate) fn set_base(&mut self, base: &str) -> Result<(), EnsureError> {
        self.transport.set_base(base).map_err(map_host)?;
        base.clone_into(&mut self.base);
        Ok(())
    }
}

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

fn poll_available(link: &mut Link, session: &QueueSession) -> Result<bool, EnsureError> {
    let (saved, path) = point_at_queue(link, &session.message_queue_url)?;
    let advertised = crate::launch::advertise_capacity(crate::launch::job_capacity());
    let polled = poll_path(link, session, &path, advertised, 0);
    restore_base(link, saved)?;
    let polled = polled?;
    Ok(matches!(offer(&polled), Offer::Acquire { .. }))
}

pub(crate) fn point_at_queue(
    link: &mut Link,
    url: &str,
) -> Result<(Option<String>, String), EnsureError> {
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

pub(crate) fn map_host(error: HostError) -> EnsureError {
    match error {
        HostError::Endpoint => EnsureError::Endpoint,
        _ => EnsureError::Unexpected {
            status: 0,
            step: "endpoint",
        },
    }
}

#[must_use]
pub(crate) fn annotate(error: SessionError, step: &'static str) -> EnsureError {
    match error {
        SessionError::Wire(WireError::Malformed) => EnsureError::Unexpected { status: 0, step },
        other => map_listen(other),
    }
}

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
