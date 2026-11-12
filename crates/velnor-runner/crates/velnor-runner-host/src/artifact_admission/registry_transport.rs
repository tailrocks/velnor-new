// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::error::HostError;
use reqwest::{Client, header};
use std::{future::Future, pin::Pin};

pub trait RegistryTransport: Send + Sync {
    fn get(
        &self,
        request: RegistryRequest,
    ) -> Pin<Box<dyn Future<Output = Result<RegistryResponse, HostError>> + Send + '_>>;
}

pub struct RegistryRequest {
    pub url: String,
    pub accept_manifest: bool,
    pub bearer: Option<String>,
    pub body_limit: usize,
}

impl std::fmt::Debug for RegistryRequest {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("RegistryRequest")
            .field("accept_manifest", &self.accept_manifest)
            .field("body_limit", &self.body_limit)
            .finish_non_exhaustive()
    }
}

pub struct RegistryResponse {
    pub status: u16,
    pub challenge: Option<String>,
    pub location: Option<String>,
    pub body: Vec<u8>,
}

impl std::fmt::Debug for RegistryResponse {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("RegistryResponse")
            .field("status", &self.status)
            .field("body_len", &self.body.len())
            .finish_non_exhaustive()
    }
}

pub(super) struct AnonymousRegistryTransport(pub(super) Client);

impl RegistryTransport for AnonymousRegistryTransport {
    fn get(
        &self,
        request: RegistryRequest,
    ) -> Pin<Box<dyn Future<Output = Result<RegistryResponse, HostError>> + Send + '_>> {
        Box::pin(async move {
            let mut call = self.0.get(&request.url);
            if request.accept_manifest {
                call = call.header(header::ACCEPT, "application/vnd.oci.image.manifest.v1+json");
            }
            if let Some(token) = request.bearer {
                call = call.bearer_auth(token);
            }
            let response = call.send().await.map_err(|_| HostError::Identity)?;
            let status = response.status().as_u16();
            let read_header = |name| {
                response
                    .headers()
                    .get(name)
                    .map(|value| {
                        value
                            .to_str()
                            .map(str::to_owned)
                            .map_err(|_| HostError::Identity)
                    })
                    .transpose()
            };
            let challenge = read_header(header::WWW_AUTHENTICATE)?;
            let location = read_header(header::LOCATION)?;
            let body = if status == 200 {
                super::github::read_limited(response, request.body_limit).await?
            } else {
                Vec::new()
            };
            Ok(RegistryResponse {
                status,
                challenge,
                location,
                body,
            })
        })
    }
}
