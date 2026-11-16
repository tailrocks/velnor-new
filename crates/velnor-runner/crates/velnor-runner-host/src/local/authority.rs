// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::net::SocketAddr;
use std::time::Duration;

use bytes::Bytes;
use http_body_util::{BodyExt, Full, Limited};
use hyper::header::{AUTHORIZATION, CONNECTION, CONTENT_TYPE, HOST};
use hyper::{Method, Request};
use hyper_util::rt::TokioIo;
use serde::Deserialize;
use tokio::net::TcpStream;

use super::plan::{LoopbackAuthority, Side};

const MAX_ANSWER_BYTES: usize = 1024 * 1024;
const MINT_TIMEOUT: Duration = Duration::from_secs(240);
const LOOKUP_TIMEOUT: Duration = Duration::from_secs(10);
const DELETE_TIMEOUT: Duration = Duration::from_secs(60);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum CallError {
    Unreachable,
    Refused(u16),
    CleanedUp,
    Malformed,
}

#[derive(Deserialize)]
struct Refusal {
    #[serde(default)]
    cleaned_up: bool,
}

#[derive(Deserialize)]
pub(super) struct Minted {
    pub(super) name: String,
    pub(super) jit: String,
}

#[derive(Debug, Clone, Deserialize)]
pub(super) struct RunAnswer {
    pub(super) id: u64,
    pub(super) attempt: u32,
    pub(super) event: String,
    #[serde(rename = "ref")]
    pub(super) git_ref: String,
    pub(super) sha: String,
    pub(super) workflow: String,
    pub(super) jobs: Vec<String>,
    pub(super) ledger: Option<String>,
    pub(super) refusal: Option<String>,
}

#[derive(Deserialize)]
struct Deleted {
    deleted: bool,
}

pub(super) struct Client {
    address: SocketAddr,
    token: String,
}

impl std::fmt::Debug for Client {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Client")
            .field("address", &self.address)
            .finish_non_exhaustive()
    }
}

impl Client {
    pub(super) fn new(authority: LoopbackAuthority, token: String) -> Self {
        Self {
            address: authority.address(),
            token,
        }
    }

    pub(super) async fn mint(&self, side: Side, name: &str) -> Result<Minted, CallError> {
        let body = format!("{{\"side\":\"{}\",\"name\":\"{name}\"}}", side.name()).into_bytes();
        let answer = self
            .send(Method::POST, "/jit".to_owned(), body, MINT_TIMEOUT)
            .await?;
        serde_json::from_slice(&answer).map_err(|_| CallError::Malformed)
    }

    pub(super) async fn run(
        &self,
        side: Side,
        id: u64,
        attempt: u32,
    ) -> Result<Option<RunAnswer>, CallError> {
        let path = format!("/runs/{}/{id}/{attempt}", side.name());
        match self
            .send(Method::GET, path, Vec::new(), LOOKUP_TIMEOUT)
            .await
        {
            Ok(answer) => serde_json::from_slice(&answer)
                .map(Some)
                .map_err(|_| CallError::Malformed),
            Err(CallError::Refused(404)) => Ok(None),
            Err(error) => Err(error),
        }
    }

    pub(super) async fn delete_agent(&self, side: Side, name: &str) -> Result<bool, CallError> {
        let path = format!("/agents/{}/{name}", side.name());
        let answer = self
            .send(Method::DELETE, path, Vec::new(), DELETE_TIMEOUT)
            .await?;
        let deleted: Deleted = serde_json::from_slice(&answer).map_err(|_| CallError::Malformed)?;
        Ok(deleted.deleted)
    }

    async fn send(
        &self,
        method: Method,
        path: String,
        body: Vec<u8>,
        limit: Duration,
    ) -> Result<Bytes, CallError> {
        tokio::time::timeout(limit, self.exchange(method, path, body))
            .await
            .map_err(|_| CallError::Unreachable)?
    }

    async fn exchange(
        &self,
        method: Method,
        path: String,
        body: Vec<u8>,
    ) -> Result<Bytes, CallError> {
        let stream = TcpStream::connect(self.address)
            .await
            .map_err(|_| CallError::Unreachable)?;
        let (mut sender, connection) = hyper::client::conn::http1::handshake(TokioIo::new(stream))
            .await
            .map_err(|_| CallError::Unreachable)?;
        let connection = tokio::spawn(connection);
        let request = Request::builder()
            .method(method)
            .uri(path)
            .header(HOST, self.address.to_string())
            .header(AUTHORIZATION, format!("Bearer {}", self.token))
            .header(CONTENT_TYPE, "application/json")
            .header(CONNECTION, "close")
            .body(Full::new(Bytes::from(body)))
            .map_err(|_| CallError::Malformed)?;
        let outcome = async {
            let response = sender
                .send_request(request)
                .await
                .map_err(|_| CallError::Unreachable)?;
            let status = response.status().as_u16();
            let bytes = Limited::new(response.into_body(), MAX_ANSWER_BYTES)
                .collect()
                .await
                .map_err(|_| CallError::Malformed)?
                .to_bytes();
            if status == 200 {
                Ok(bytes)
            } else if status == 502
                && serde_json::from_slice::<Refusal>(&bytes).is_ok_and(|body| body.cleaned_up)
            {
                Err(CallError::CleanedUp)
            } else {
                Err(CallError::Refused(status))
            }
        }
        .await;
        connection.abort();
        outcome
    }
}
