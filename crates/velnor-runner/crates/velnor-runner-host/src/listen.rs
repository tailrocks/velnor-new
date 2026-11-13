// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::cell::Cell;

use velnor_runner_github::{
    AdminConnection, Poll, QueueSession, RefreshGate, SessionError, Transport, WireError, poll,
    workflow_run::VerifiedRepositoryScope,
};

use zeroize::Zeroize;

use crate::config::RepositoryConnection;
use crate::error::HostError;
use crate::https::HttpsTransport;
use crate::scale_set::EnsureError;

pub(crate) const OWNER_NAME: &str = "velnor-host";
pub(crate) const GITHUB_API: &str = "https://api.github.com";

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

impl std::fmt::Debug for Secret {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_tuple("Secret")
            .field(&"[REDACTED]")
            .finish()
    }
}

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
    #[must_use]
    pub(crate) fn token(&self) -> &str {
        self.admin.expose_token()
    }

    #[must_use]
    pub(crate) fn admin_origin(&self) -> &str {
        self.admin.expose_url()
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

    pub(crate) fn refresh_admin(
        &mut self,
        binding: &RepositoryConnection,
        pat: &str,
    ) -> Result<(), HostError> {
        let (owner, repo) = binding
            .repository
            .split_once('/')
            .ok_or(HostError::Config)?;
        let mut transport = HttpsTransport::new(GITHUB_API)?;
        let registration = velnor_runner_github::registration_token(
            &mut transport,
            &velnor_runner_github::RegistrationTokenCall {
                scope: velnor_runner_github::RegistrationScope::Repository { owner, repo },
                pat,
            },
        )
        .map_err(|_| HostError::Endpoint)?;
        let config_url = format!("https://github.com/{owner}/{repo}");
        let admin = velnor_runner_github::admin_connection(
            &mut transport,
            &velnor_runner_github::AdminConnectionCall {
                config_url: &config_url,
                registration_token: registration.expose(),
            },
        )
        .map_err(|_| HostError::Endpoint)?;
        let base = admin.expose_url().to_owned();
        self.transport.set_base(&base)?;
        self.admin = admin;
        self.base = base;
        Ok(())
    }
}

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

pub(crate) fn poll_path(
    link: &mut Link,
    session: &QueueSession,
    path: &str,
    total_capacity: u32,
    cursor: i64,
) -> Result<PollPathResult, EnsureError> {
    poll_path_with(
        &mut link.transport,
        session.token(),
        path,
        total_capacity,
        cursor,
    )
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum PollPathResult {
    Response(Poll),

    RefreshRequired,
}

pub(crate) fn poll_path_with<T: Transport + ?Sized>(
    transport: &mut T,
    queue_token: &str,
    path: &str,
    total_capacity: u32,
    cursor: i64,
) -> Result<PollPathResult, EnsureError> {
    let gate = RefreshGate::new();
    let refresh_requested = Cell::new(false);
    let refresh = || {
        refresh_requested.set(true);
        Err(WireError::RefreshExhausted)
    };
    match poll(
        transport,
        path,
        cursor,
        total_capacity,
        queue_token,
        &gate,
        refresh,
    ) {
        Ok(response) => Ok(PollPathResult::Response(response)),
        Err(SessionError::Wire(WireError::RefreshExhausted)) if refresh_requested.get() => {
            Ok(PollPathResult::RefreshRequired)
        }
        Err(error) => Err(annotate(error, "poll")),
    }
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
        SessionError::Uncertain | SessionError::Wire(WireError::RefreshExhausted) => {
            EnsureError::Uncertain
        }
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
