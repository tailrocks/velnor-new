// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0
// SPDX-FileContributor: tailrocks/velnor-legacy@1b9fc0388925d21ff44ba1be5027684c38c8e4c9 crates/velnor-runner/src/protocol.rs

use std::sync::PoisonError;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use reqwest::{Method, StatusCode};
use serde::Serialize;
use serde::de::DeserializeOwned;

use super::retry::{Failure, Mode, Notes, attempts, retry_after, rpc};
use super::wire::{
    CreateRequest, CreateResponse, FinalizeRequest, FinalizeResponse, ListRequest, ListResponse,
    NamedRequest, Scope, SignedUrlResponse,
};
use crate::exec::{ResultsScope, StepContext};
use crate::protocol::http::{
    ReadError, SendError, github_json_http_response_on, idle_http_client,
    validate_known_service_url,
};
use crate::protocol::results::KNOWN_HOSTS;
use crate::secret::Secret;

const SERVICE: &str = "twirp/github.actions.results.api.v1.ArtifactService";
const CALL_TIMEOUT: Duration = Duration::from_secs(30);
const SOCKET_IDLE: Duration = Duration::from_secs(180);
const CALL_TIMEOUT_ENV: &str = "VELNOR_ARTIFACT_CALL_TIMEOUT_MS";
const QUOTA_MESSAGE: &str = "Artifact storage quota has been hit. Unable to upload any new artifacts.\nMore info on storage limits: https://docs.github.com/en/billing/managing-billing-for-github-actions/about-billing-for-github-actions#calculating-minute-and-storage-spending";
const NETWORK_HELP: &str = "If you are using self-hosted runners, please make sure your runner has access to all GitHub endpoints: https://docs.github.com/en/actions/hosting-your-own-runners/managing-self-hosted-runners/about-self-hosted-runners#communication-between-self-hosted-runners-and-github";
const CONTENT_TYPE_ZIP: &str = "application/zip";
const CREATE_VERSION: u8 = 7;

#[derive(Debug)]
pub(super) struct TwirpFailure {
    status: u16,
    code: String,
    msg: Option<String>,
}

impl std::fmt::Display for TwirpFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let reason = StatusCode::from_u16(self.status)
            .ok()
            .and_then(|status| status.canonical_reason())
            .unwrap_or_default();
        write!(f, "Failed request: ({}) {reason}", self.status)?;
        match self.msg.as_deref() {
            Some(msg) if !msg.is_empty() => write!(f, ": {msg}"),
            _ => Ok(()),
        }
    }
}

impl std::error::Error for TwirpFailure {}

impl TwirpFailure {
    pub(super) fn is_already_exists(error: &anyhow::Error) -> bool {
        error
            .downcast_ref::<Self>()
            .is_some_and(|failure| failure.code == "already_exists")
    }
}

fn transport_failure(error: anyhow::Error) -> Failure {
    if let Some(read) = error.downcast_ref::<ReadError>() {
        let after = (read.status == 429)
            .then(|| retry_after(read.headers.expose()))
            .flatten();
        return Failure {
            retryable: true,
            after,
            error,
        };
    }
    match error
        .downcast_ref::<SendError>()
        .and_then(|send| send.network_code)
    {
        Some(code) => Failure::fatal(anyhow::anyhow!(network_message(code))),
        None => Failure::transient(error),
    }
}

fn timeout_or(default: Duration) -> Duration {
    if cfg!(feature = "test-support") {
        std::env::var(CALL_TIMEOUT_ENV)
            .ok()
            .and_then(|text| text.parse::<u64>().ok())
            .map_or(default, Duration::from_millis)
    } else {
        default
    }
}

pub(super) fn call_timeout() -> Duration {
    timeout_or(CALL_TIMEOUT)
}

fn network_message(code: &str) -> String {
    format!("Unable to make request: {code}\n{NETWORK_HELP}")
}

#[derive(Debug, Clone)]
pub(super) struct Listed {
    pub(super) run_backend_id: String,
    pub(super) job_backend_id: String,
    pub(super) id: u64,
    pub(super) name: String,
    pub(super) digest: Option<String>,
}

#[derive(Debug)]
pub(super) struct Client {
    base: String,
    token: Secret<String>,
    run: String,
    job: String,
    notes: Notes,
    http: reqwest::Client,
}

impl Client {
    pub(super) fn new(scope: &ResultsScope) -> Result<Self> {
        let url = validate_known_service_url(&scope.url, "ResultsServiceUrl", KNOWN_HOSTS)?;
        Ok(Self {
            base: url.origin().ascii_serialization(),
            token: Secret::new(scope.token.expose().clone()),
            run: scope.run_backend_id.clone(),
            job: scope.job_backend_id.clone(),
            notes: Notes::default(),
            http: idle_http_client(timeout_or(SOCKET_IDLE))?,
        })
    }

    pub(super) fn drain_notes(&self, ctx: &mut StepContext<'_>) {
        let lines = std::mem::take(&mut *self.notes.lock().unwrap_or_else(PoisonError::into_inner));
        for line in lines {
            ctx.log(&line);
        }
    }

    fn scope(&self) -> Scope<'_> {
        Scope {
            workflow_run_backend_id: &self.run,
            workflow_job_run_backend_id: &self.job,
        }
    }

    async fn call<T: DeserializeOwned>(
        &self,
        method: &'static str,
        request: &impl Serialize,
    ) -> Result<T> {
        let body = serde_json::to_string(request).with_context(|| format!("serialize {method}"))?;
        let url = format!("{}/{SERVICE}/{method}", self.base);
        let value = attempts(rpc(), Mode::Rpc(&self.notes), || async {
            async {
                let response = github_json_http_response_on(
                    &self.http,
                    Method::POST,
                    &url,
                    self.token.expose(),
                    Some(body.clone()),
                    None,
                )
                .await
                .map_err(transport_failure)?;
                let status = response.status;
                let value = serde_json::from_str::<serde_json::Value>(response.body.expose())
                    .map_err(|error| Failure {
                        retryable: true,
                        after: (status == 429)
                            .then(|| retry_after(response.headers.expose()))
                            .flatten(),
                        error: anyhow::Error::new(error).context(format!(
                            "{method}: response body is not JSON, status={status}"
                        )),
                    })?;
                if (200..300).contains(&status) {
                    return Ok(value);
                }
                let message = value.get("msg").and_then(serde_json::Value::as_str);
                if message.is_some_and(|text| text.contains("insufficient usage")) {
                    return Err(Failure::fatal(anyhow::anyhow!(QUOTA_MESSAGE)));
                }
                let code = value
                    .get("code")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or_default()
                    .to_owned();
                let failure = TwirpFailure {
                    status,
                    code,
                    msg: message.map(str::to_owned),
                };
                let retryable = matches!(status, 429 | 500 | 502 | 503 | 504);
                Err(Failure {
                    retryable,
                    after: (status == 429)
                        .then(|| retry_after(response.headers.expose()))
                        .flatten(),
                    error: if retryable {
                        failure.into()
                    } else {
                        anyhow::Error::new(failure).context("Received non-retryable error")
                    },
                })
            }
            .await
        })
        .await
        .map_err(|error| error.context(format!("Failed to {method}")))?;
        serde_json::from_value(value).with_context(|| format!("parse {method} response"))
    }

    pub(super) async fn create(&self, name: &str, expires_at: Option<&str>) -> Result<String> {
        let request = CreateRequest {
            scope: self.scope(),
            name,
            expires_at,
            version: CREATE_VERSION,
            mime_type: CONTENT_TYPE_ZIP,
        };
        let response: CreateResponse = self.call("CreateArtifact", &request).await?;
        if !response.ok || response.signed_upload_url.is_empty() {
            bail!("CreateArtifact: response from backend was not ok");
        }
        Ok(response.signed_upload_url)
    }

    pub(super) async fn finalize(&self, name: &str, size: u64, sha256: &str) -> Result<u64> {
        let request = FinalizeRequest {
            scope: self.scope(),
            name,
            size: size.to_string(),
            hash: format!("sha256:{sha256}"),
        };
        let response: FinalizeResponse = self.call("FinalizeArtifact", &request).await?;
        if !response.ok || response.artifact_id == 0 {
            bail!("FinalizeArtifact: response from backend was not ok");
        }
        Ok(response.artifact_id)
    }

    pub(super) async fn list(&self, name_filter: Option<&str>) -> Result<Vec<Listed>> {
        let request = ListRequest {
            scope: self.scope(),
            name_filter,
        };
        let response: ListResponse = self.call("ListArtifacts", &request).await?;
        Ok(response
            .artifacts
            .into_iter()
            .map(|wire| Listed {
                run_backend_id: wire.workflow_run_backend_id,
                job_backend_id: wire.workflow_job_run_backend_id,
                id: wire.database_id,
                name: wire.name,
                digest: wire.digest.filter(|digest| !digest.is_empty()),
            })
            .collect())
    }

    pub(super) async fn signed_url(&self, artifact: &Listed) -> Result<String> {
        let request = NamedRequest {
            scope: Scope {
                workflow_run_backend_id: &artifact.run_backend_id,
                workflow_job_run_backend_id: &artifact.job_backend_id,
            },
            name: &artifact.name,
        };
        let response: SignedUrlResponse = self.call("GetSignedArtifactURL", &request).await?;
        if response.signed_url.is_empty() {
            bail!(
                "GetSignedArtifactURL: empty signed URL for '{}'",
                artifact.name
            );
        }
        Ok(response.signed_url)
    }

    pub(super) async fn delete(&self, artifact: &Listed) -> Result<()> {
        let request = NamedRequest {
            scope: Scope {
                workflow_run_backend_id: &artifact.run_backend_id,
                workflow_job_run_backend_id: &artifact.job_backend_id,
            },
            name: &artifact.name,
        };
        let _: serde_json::Value = self.call("DeleteArtifact", &request).await?;
        Ok(())
    }
}
