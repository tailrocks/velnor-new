// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::{OrchestratorError, internal::internal};
use std::{
    io::{self, Read},
    sync::Arc,
};
use velnor_actions_mise::CheckDeadline;

pub struct RegistryRequest {
    pub method: String,
    pub url: String,
    pub headers: Vec<(String, String)>,
}

impl std::fmt::Debug for RegistryRequest {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("RegistryRequest")
            .field("method", &self.method)
            .finish_non_exhaustive()
    }
}

pub struct RegistryResponse {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: Box<dyn Read + Send>,
}

impl std::fmt::Debug for RegistryResponse {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("RegistryResponse")
            .field("status", &self.status)
            .finish_non_exhaustive()
    }
}

pub trait RegistryIo: Send + Sync {
    fn request(
        &self,
        request: RegistryRequest,
        body: &mut dyn Read,
    ) -> io::Result<RegistryResponse>;
}

pub(super) struct UreqIo {
    deadline: CheckDeadline,
}

impl UreqIo {
    pub(super) fn new(deadline: CheckDeadline) -> Self {
        Self { deadline }
    }
    fn agent(&self) -> io::Result<ureq::Agent> {
        let trust = rustls_native_certs::load_native_certs();
        if !trust.errors.is_empty() || trust.certs.is_empty() {
            return Err(io::ErrorKind::Other.into());
        }
        let roots = trust
            .certs
            .iter()
            .map(|certificate| ureq::tls::Certificate::from_der(certificate.as_ref()).to_owned())
            .collect::<Vec<_>>();
        let tls = ureq::tls::TlsConfig::builder()
            .root_certs(roots.into())
            .unversioned_rustls_crypto_provider(Arc::new(rustls::crypto::ring::default_provider()))
            .build();
        let timeout = self
            .deadline
            .remaining()
            .map_err(|_| io::ErrorKind::TimedOut)?;
        let short = timeout.min(std::time::Duration::from_secs(30));
        let config = ureq::Agent::config_builder()
            .tls_config(tls)
            .proxy(None)
            .timeout_global(Some(timeout))
            .timeout_resolve(Some(short))
            .timeout_connect(Some(short))
            .timeout_recv_response(Some(short))
            .max_response_header_size(32 * 1024)
            .http_status_as_error(false)
            .max_redirects(0)
            .build();
        Ok(ureq::Agent::new_with_config(config))
    }
}

impl RegistryIo for UreqIo {
    fn request(
        &self,
        request: RegistryRequest,
        body: &mut dyn Read,
    ) -> io::Result<RegistryResponse> {
        let mut builder = ureq::http::Request::builder()
            .method(request.method.as_str())
            .uri(&request.url);
        for (name, value) in &request.headers {
            builder = builder.header(name, value);
        }
        let agent = self.agent()?;
        let response = if request.method == "GET" || request.method == "HEAD" {
            let request = builder.body(()).map_err(io::Error::other)?;
            agent.run(request).map_err(io::Error::other)?
        } else {
            let request = builder
                .body(ureq::SendBody::from_reader(body))
                .map_err(io::Error::other)?;
            agent.run(request).map_err(io::Error::other)?
        };
        let (parts, body) = response.into_parts();
        let headers = parts
            .headers
            .iter()
            .map(|(name, value)| {
                Ok((
                    name.to_string(),
                    value.to_str().map_err(io::Error::other)?.to_owned(),
                ))
            })
            .collect::<io::Result<Vec<_>>>()?;
        Ok(RegistryResponse {
            status: parts.status.as_u16(),
            headers,
            body: Box::new(body.into_reader()),
        })
    }
}

struct TokenPayload(std::collections::BTreeMap<String, serde_json::Value>);

impl<'de> serde::Deserialize<'de> for TokenPayload {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct Visitor;
        impl<'de> serde::de::Visitor<'de> for Visitor {
            type Value = TokenPayload;
            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("a token response object")
            }
            fn visit_map<M: serde::de::MapAccess<'de>>(
                self,
                mut map: M,
            ) -> Result<Self::Value, M::Error> {
                let mut values = std::collections::BTreeMap::new();
                while let Some((key, value)) = map.next_entry::<String, serde_json::Value>()? {
                    if values.insert(key, value).is_some() {
                        return Err(serde::de::Error::custom("duplicate token response key"));
                    }
                }
                Ok(TokenPayload(values))
            }
        }
        deserializer.deserialize_map(Visitor)
    }
}

pub(super) struct Transport {
    authorization: String,
    repository: String,
    io: Arc<dyn RegistryIo>,
    token: std::sync::Mutex<Option<String>>,
}

impl Transport {
    pub(super) fn new(authorization: String, repository: String, io: Arc<dyn RegistryIo>) -> Self {
        Self {
            authorization,
            repository,
            io,
            token: std::sync::Mutex::new(None),
        }
    }

    fn token(&self) -> Result<String, OrchestratorError> {
        let mut cached = self
            .token
            .lock()
            .map_err(|_| internal("image_publication_registry_token"))?;
        if let Some(token) = cached.as_ref() {
            return Ok(token.clone());
        }
        let url = format!(
            "https://ghcr.io/token?service=ghcr.io&scope=repository%3A{}%3Apull%2Cpush",
            self.repository
        );
        let request = RegistryRequest {
            method: "GET".into(),
            url,
            headers: vec![
                (
                    "authorization".into(),
                    format!("Basic {}", self.authorization),
                ),
                ("accept-encoding".into(), "identity".into()),
            ],
        };
        let mut response = self
            .io
            .request(request, &mut io::empty())
            .map_err(|_| internal("image_publication_registry_token"))?;
        if response.status != 200 {
            return Err(internal("image_publication_registry_token"));
        }
        let mut bytes = Vec::new();
        response
            .body
            .by_ref()
            .take(65_537)
            .read_to_end(&mut bytes)
            .map_err(|_| internal("image_publication_registry_token"))?;
        if bytes.len() > 65_536 {
            return Err(internal("image_publication_registry_token"));
        }
        let TokenPayload(value) = serde_json::from_slice(&bytes)
            .map_err(|_| internal("image_publication_registry_token"))?;
        let token = value
            .get("token")
            .or_else(|| value.get("access_token"))
            .and_then(serde_json::Value::as_str)
            .filter(|token| {
                (16..=8192).contains(&token.len())
                    && token
                        .bytes()
                        .all(|byte| byte.is_ascii_alphanumeric() || b"._~=-".contains(&byte))
            })
            .ok_or_else(|| internal("image_publication_registry_token"))?;
        *cached = Some(token.to_owned());
        Ok(token.to_owned())
    }

    pub(super) fn send(
        &self,
        method: &str,
        path: &str,
        headers: &[(String, String)],
        body: &mut dyn Read,
    ) -> Result<RegistryResponse, OrchestratorError> {
        let token = self.token()?;
        let mut headers = headers.to_vec();
        headers.push(("authorization".into(), format!("Bearer {token}")));
        headers.push(("accept-encoding".into(), "identity".into()));
        let request = RegistryRequest {
            method: method.into(),
            url: format!("https://ghcr.io{path}"),
            headers,
        };
        self.io
            .request(request, body)
            .map_err(|_| internal("image_publication_registry_request"))
    }

    pub(super) fn cdn(&self, url: &str) -> Result<RegistryResponse, OrchestratorError> {
        let request = RegistryRequest {
            method: "GET".into(),
            url: url.into(),
            headers: vec![("accept-encoding".into(), "identity".into())],
        };
        self.io
            .request(request, &mut io::empty())
            .map_err(|_| internal("image_publication_registry_request"))
    }
}
