// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde::Deserialize;
use serde_json::json;

use crate::error::CiError;
use crate::io::{Body, Request, Response, Transport};

pub const HUB_URL: &str = "https://hub.docker.com";
const LIMIT: u64 = 1 << 20;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Preflight {
    pub sha_exists: bool,
    pub report: String,
}

#[derive(Deserialize)]
struct Token {
    access_token: String,
}

#[derive(Deserialize)]
struct Repository {
    is_private: bool,
}

fn segment(text: &str) -> bool {
    !text.is_empty()
        && text.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'.' | b'_' | b'-')
        })
}

fn tag_text(text: &str) -> bool {
    !text.is_empty()
        && text.len() <= 128
        && text
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
}

fn send(
    transport: &dyn Transport,
    method: &'static str,
    url: String,
    headers: Vec<(&'static str, String)>,
    body: Body<'_>,
) -> Result<(u16, Vec<u8>), CiError> {
    let response: Response = transport.send(Request {
        method,
        url,
        headers,
        body,
    })?;
    let status = response.status;
    Ok((status, response.bounded(LIMIT)?))
}

fn token(
    transport: &dyn Transport,
    base: &str,
    username: &str,
    secret: &str,
) -> Result<String, CiError> {
    let body = json!({"identifier": username, "secret": secret}).to_string();
    let (status, bytes) = send(
        transport,
        "POST",
        format!("{base}/v2/auth/token"),
        vec![("Content-Type", "application/json".to_owned())],
        Body::Bytes(body.as_bytes()),
    )?;
    if status != 200 {
        return Err(CiError::new(format!(
            "Docker Hub refused the login with HTTP {status}"
        )));
    }
    serde_json::from_slice::<Token>(&bytes)
        .map(|token| token.access_token)
        .map_err(|_| CiError::new("Docker Hub answered the login without an access token"))
}

pub fn preflight(
    transport: &dyn Transport,
    base: &str,
    username: &str,
    secret: &str,
    repository: &str,
    tag: &str,
) -> Result<Preflight, CiError> {
    let valid = repository
        .split_once('/')
        .is_some_and(|(owner, name)| segment(owner) && segment(name) && !name.contains('/'));
    if !valid || !tag_text(tag) {
        return Err(CiError::new(format!(
            "`{repository}:{tag}` is not a Docker Hub repository and tag"
        )));
    }
    if username.is_empty() || secret.is_empty() {
        return Err(CiError::new(
            "VELNOR_HUB_USERNAME and VELNOR_HUB_TOKEN must both be set",
        ));
    }
    let (namespace, name) = repository.split_once('/').ok_or_else(|| {
        CiError::new(format!(
            "Docker Hub repository {repository} is not namespace/name"
        ))
    })?;
    let bearer = format!("Bearer {}", token(transport, base, username, secret)?);
    let authorized = || vec![("Authorization", bearer.clone())];
    let (status, bytes) = send(
        transport,
        "GET",
        format!("{base}/v2/namespaces/{namespace}/repositories/{name}"),
        authorized(),
        Body::Empty,
    )?;
    match status {
        200 => {}
        404 => {
            return Err(CiError::new(format!(
                "Docker Hub repository {repository} does not exist, and a push would create it public; create it as a private repository first"
            )));
        }
        other => {
            return Err(CiError::new(format!(
                "Docker Hub answered the repository lookup with HTTP {other}"
            )));
        }
    }
    let listed: Repository = serde_json::from_slice(&bytes).map_err(|_| {
        CiError::new("Docker Hub answered the repository lookup without is_private")
    })?;
    if !listed.is_private {
        return Err(CiError::new(format!(
            "Docker Hub repository {repository} is public; make it private before an image is pushed"
        )));
    }
    let (status, _) = send(
        transport,
        "GET",
        format!("{base}/v2/namespaces/{namespace}/repositories/{name}/tags/{tag}"),
        authorized(),
        Body::Empty,
    )?;
    let sha_exists = match status {
        200 => true,
        404 => false,
        other => {
            return Err(CiError::new(format!(
                "Docker Hub answered the tag lookup with HTTP {other}"
            )));
        }
    };
    Ok(Preflight {
        sha_exists,
        report: format!(
            "{repository} is private; tag {tag} {}\n",
            if sha_exists {
                "already exists and this push replaces it"
            } else {
                "is new"
            }
        ),
    })
}
