// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0
// SPDX-FileContributor: tailrocks/velnor-legacy@1b9fc0388925d21ff44ba1be5027684c38c8e4c9 crates/velnor-runner/src/protocol.rs

use anyhow::{Context, Result, bail};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use ring::rand::SystemRandom;
use ring::signature::{RSA_PKCS1_SHA256, RsaKeyPair};
use serde::{Deserialize, Serialize};
use std::fmt;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use uuid::Uuid;

use super::http::{oauth_form_request, redacted_authenticated_url};
use super::jit::private_key_der_from_pem;

const OAUTH_TIMEOUT: Duration = Duration::from_secs(5);
const OAUTH_MAX_RESPONSE_BYTES: usize = 64 * 1024;
const ASSERTION_BACKDATE_SECONDS: u64 = 120;
const ASSERTION_LIFETIME_SECONDS: u64 = 300;
const CLOCK_SKEW_MARKER: &str = "Current server time is";
const JWT_HEADER: &[u8] = br#"{"alg":"RS256","typ":"JWT"}"#;

#[derive(Clone)]
pub struct OAuthJwtCredentials {
    pub client_id: String,
    pub authorization_url: String,
    pub token_url: String,
    pub private_key_pem: String,
}

impl fmt::Debug for OAuthJwtCredentials {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("OAuthJwtCredentials")
            .field("client_id", &self.client_id)
            .field(
                "authorization_url",
                &redacted_authenticated_url(&self.authorization_url),
            )
            .field("token_url", &redacted_authenticated_url(&self.token_url))
            .field("private_key_pem", &"<redacted>")
            .finish()
    }
}

#[derive(Serialize)]
struct OAuthJwtClaims {
    iss: String,
    sub: String,
    aud: String,
    jti: String,
    nbf: u64,
    exp: u64,
}

#[derive(Deserialize)]
struct OAuthTokenResponse {
    access_token: Option<String>,
    expires_in: Option<i64>,
    error: Option<String>,
    error_description: Option<String>,
}

#[derive(Clone, PartialEq, Eq)]
pub struct OAuthAccessToken {
    pub token: String,
    pub expires_in: Option<Duration>,
}

impl fmt::Debug for OAuthAccessToken {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("OAuthAccessToken")
            .field("token", &"<redacted>")
            .field("expires_in", &self.expires_in)
            .finish()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OAuthErrorKind {
    RegistrationDeleted,
    ClockSkew,
    Other,
}

#[derive(Debug)]
pub struct OAuthTokenError {
    pub kind: OAuthErrorKind,
    error: String,
    description: String,
}

impl fmt::Display for OAuthTokenError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.kind {
            OAuthErrorKind::RegistrationDeleted => write!(
                f,
                "GitHub runner registration no longer exists: {}",
                self.description
            ),
            OAuthErrorKind::ClockSkew | OAuthErrorKind::Other => write!(
                f,
                "OAuth token request failed: error={}, description={}",
                self.error, self.description
            ),
        }
    }
}

impl std::error::Error for OAuthTokenError {}

impl OAuthTokenError {
    fn new(error: String, description: String) -> Self {
        let kind = if error.eq_ignore_ascii_case("invalid_client") {
            OAuthErrorKind::RegistrationDeleted
        } else if description.contains(CLOCK_SKEW_MARKER) {
            OAuthErrorKind::ClockSkew
        } else {
            OAuthErrorKind::Other
        };
        Self {
            kind,
            error,
            description,
        }
    }
}

#[must_use]
pub fn oauth_error_kind(error: &anyhow::Error) -> Option<OAuthErrorKind> {
    error
        .chain()
        .find_map(|cause| cause.downcast_ref::<OAuthTokenError>())
        .map(|oauth| oauth.kind)
}

#[derive(Debug, Clone, Copy, Default)]
pub struct OAuthClient;

impl OAuthClient {
    pub fn new() -> Result<Self> {
        Ok(Self)
    }

    pub async fn exchange_client_credentials(
        &self,
        credentials: &OAuthJwtCredentials,
    ) -> Result<OAuthAccessToken> {
        let assertion = build_client_assertion(credentials)?;
        let body = url::form_urlencoded::Serializer::new(String::new())
            .append_pair("grant_type", "client_credentials")
            .append_pair(
                "client_assertion_type",
                "urn:ietf:params:oauth:client-assertion-type:jwt-bearer",
            )
            .append_pair("client_assertion", &assertion)
            .finish();
        let (status, text) =
            oauth_form_request(&credentials.token_url, body, OAUTH_TIMEOUT).await?;
        parse_oauth_token_response(status, &text)
    }
}

fn parse_oauth_token_response(status: u16, body: &str) -> Result<OAuthAccessToken> {
    if body.len() > OAUTH_MAX_RESPONSE_BYTES {
        bail!("OAuth token response exceeded {OAUTH_MAX_RESPONSE_BYTES} bytes");
    }
    if status != 200 && status != 400 {
        bail!("OAuth token request failed: HTTP status {status}");
    }
    let response: OAuthTokenResponse =
        serde_json::from_str(body.trim()).context("parse OAuth token response")?;
    let token = response.access_token.filter(|value| !value.is_empty());
    if let (200, Some(token)) = (status, token) {
        return Ok(OAuthAccessToken {
            token,
            expires_in: response
                .expires_in
                .and_then(|seconds| u64::try_from(seconds).ok())
                .filter(|seconds| *seconds > 0)
                .map(Duration::from_secs),
        });
    }
    if let Some(error) = response.error {
        let description = response.error_description.unwrap_or_default();
        return Err(OAuthTokenError::new(error, description).into());
    }
    if status != 200 {
        bail!("OAuth token request failed: HTTP status {status}");
    }
    bail!("OAuth token response missing access_token")
}

pub fn build_client_assertion(credentials: &OAuthJwtCredentials) -> Result<String> {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .context("system clock before unix epoch")?
        .as_secs();
    let not_before = now.saturating_sub(ASSERTION_BACKDATE_SECONDS);
    let claims = OAuthJwtClaims {
        iss: credentials.client_id.clone(),
        sub: credentials.client_id.clone(),
        aud: credentials.authorization_url.clone(),
        jti: Uuid::new_v4().to_string(),
        nbf: not_before,
        exp: not_before + ASSERTION_LIFETIME_SECONDS,
    };
    let key = RsaKeyPair::from_der(&private_key_der_from_pem(&credentials.private_key_pem)?)
        .map_err(|error| anyhow::anyhow!("load runner RSA private key: {error}"))?;
    let signing_input = format!(
        "{}.{}",
        URL_SAFE_NO_PAD.encode(JWT_HEADER),
        URL_SAFE_NO_PAD.encode(serde_json::to_vec(&claims).context("serialize OAuth claims")?)
    );
    let mut signature = vec![0_u8; key.public().modulus_len()];
    key.sign(
        &RSA_PKCS1_SHA256,
        &SystemRandom::new(),
        signing_input.as_bytes(),
        &mut signature,
    )
    .map_err(|error| anyhow::anyhow!("sign OAuth client assertion: {error}"))?;
    Ok(format!(
        "{signing_input}.{}",
        URL_SAFE_NO_PAD.encode(signature)
    ))
}
