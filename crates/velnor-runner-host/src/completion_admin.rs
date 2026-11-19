// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::sync::{Arc, RwLock};

use velnor_runner_github::{RunnerReference, get_runner_by_name, remove_runner};

use crate::{error::HostError, https::HttpsTransport, listen::Secret};

#[derive(Debug)]
struct Credentials {
    base: String,
    token: Secret,
}

#[derive(Clone, Debug)]
pub struct CompletionAdminClient {
    credentials: Arc<RwLock<Credentials>>,
}

#[derive(Debug)]
pub struct CompletionAdminSession {
    transport: HttpsTransport,
    token: Secret,
}

impl CompletionAdminClient {
    pub fn new(base: &str, token: &str) -> Result<Self, HostError> {
        validate(base, token)?;
        Ok(Self {
            credentials: Arc::new(RwLock::new(Credentials {
                base: base.to_owned(),
                token: Secret::new(token),
            })),
        })
    }

    pub fn refresh(&self, base: &str, token: &str) -> Result<(), HostError> {
        validate(base, token)?;
        let mut guard = self.credentials.write().map_err(|_| HostError::Journal)?;
        *guard = Credentials {
            base: base.to_owned(),
            token: Secret::new(token),
        };
        Ok(())
    }

    pub fn session(&self) -> Result<CompletionAdminSession, HostError> {
        let guard = self.credentials.read().map_err(|_| HostError::Journal)?;
        Ok(CompletionAdminSession {
            transport: HttpsTransport::new(&guard.base)?,
            token: Secret::new(guard.token.expose()),
        })
    }
}

impl CompletionAdminSession {
    pub fn lookup(&mut self, name: &str) -> Result<Option<RunnerReference>, HostError> {
        get_runner_by_name(&mut self.transport, name, self.token.expose())
            .map_err(|_| HostError::Endpoint)
    }

    pub fn remove(&mut self, id: i64) -> Result<(), HostError> {
        remove_runner(&mut self.transport, id, self.token.expose()).map_err(|_| HostError::Endpoint)
    }
}

fn validate(base: &str, token: &str) -> Result<(), HostError> {
    if token.is_empty() {
        return Err(HostError::EmptySecret);
    }
    if token.chars().any(|ch| ch.is_control() || ch == '"') {
        return Err(HostError::Endpoint);
    }
    HttpsTransport::new(base)?;
    let url = reqwest::Url::parse(base).map_err(|_| HostError::Endpoint)?;
    let authority = base
        .strip_prefix("https://")
        .and_then(|rest| rest.split('/').next());
    if authority.is_none_or(|value| value.contains('@'))
        || url.scheme() != "https"
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
    {
        return Err(HostError::Endpoint);
    }
    Ok(())
}
