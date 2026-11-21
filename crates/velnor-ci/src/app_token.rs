// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::time::Duration;

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use ring::rand::SystemRandom;
use ring::signature::{RSA_PKCS1_SHA256, RsaKeyPair};
use rustls_pki_types::PrivateKeyDer;
use rustls_pki_types::pem::PemObject;
use serde::Deserialize;
use zeroize::{Zeroize, Zeroizing};

use crate::error::CiError;
use crate::release_family::GitHubPost;

pub const CLOCK_SKEW_SECONDS: u64 = 60;
pub const LIFETIME_SECONDS: u64 = 540;
const HEADER: &str = r#"{"alg":"RS256","typ":"JWT"}"#;
const UNREADABLE_KEY: &str =
    "the App private key is not a PEM RSA private key (PKCS#1 or PKCS#8) of 2048 bits or more";

pub struct AppKey(RsaKeyPair);

impl std::fmt::Debug for AppKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("AppKey(redacted)")
    }
}

impl AppKey {
    pub fn from_pem(pem: &str) -> Result<Self, CiError> {
        let text = Zeroizing::new(pem.trim().replace("\\n", "\n"));
        let mut der = PrivateKeyDer::from_pem_slice(text.as_bytes())
            .map_err(|_| CiError::new(UNREADABLE_KEY))?;
        let pair = match &der {
            PrivateKeyDer::Pkcs1(key) => RsaKeyPair::from_der(key.secret_pkcs1_der()),
            PrivateKeyDer::Pkcs8(key) => RsaKeyPair::from_pkcs8(key.secret_pkcs8_der()),
            _ => {
                der.zeroize();
                return Err(CiError::new(UNREADABLE_KEY));
            }
        };
        der.zeroize();
        pair.map(Self).map_err(|_| CiError::new(UNREADABLE_KEY))
    }
}

#[must_use]
pub fn claims(client_id: &str, now: u64) -> String {
    serde_json::json!({
        "iat": now.saturating_sub(CLOCK_SKEW_SECONDS),
        "exp": now + LIFETIME_SECONDS,
        "iss": client_id,
    })
    .to_string()
}

pub fn jwt(key: &AppKey, client_id: &str, now: u64) -> Result<Zeroizing<String>, CiError> {
    let signed = format!(
        "{}.{}",
        URL_SAFE_NO_PAD.encode(HEADER),
        URL_SAFE_NO_PAD.encode(claims(client_id, now))
    );
    let mut signature = vec![0; key.0.public().modulus_len()];
    key.0
        .sign(
            &RSA_PKCS1_SHA256,
            &SystemRandom::new(),
            signed.as_bytes(),
            &mut signature,
        )
        .map_err(|_| CiError::new("the App JWT could not be signed"))?;
    Ok(Zeroizing::new(format!(
        "{signed}.{}",
        URL_SAFE_NO_PAD.encode(signature)
    )))
}

#[must_use]
pub fn token_request(repositories: &[String]) -> String {
    serde_json::json!({
        "repositories": repositories,
        "permissions": { "contents": "read" },
    })
    .to_string()
}

#[derive(Deserialize)]
struct Installation {
    id: u64,
}

#[derive(Deserialize)]
struct Minted {
    token: String,
}

const ATTEMPTS: usize = 3;
const BACKOFF: [Duration; ATTEMPTS - 1] = [Duration::from_secs(1), Duration::from_secs(3)];

fn plain(name: &str) -> bool {
    !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
}

fn valid_owner(name: &str) -> bool {
    plain(name) && !name.starts_with('.')
}

fn valid_repository(name: &str) -> bool {
    plain(name) && !name.chars().all(|c| c == '.')
}

fn retried(
    pause: &dyn Fn(Duration),
    mut attempt: impl FnMut() -> Result<String, CiError>,
) -> Result<String, CiError> {
    let mut failure = None;
    for delay in std::iter::once(None).chain(BACKOFF.map(Some)) {
        if let Some(delay) = delay {
            pause(delay);
        }
        match attempt() {
            Err(error) if error.is_transient() => failure = Some(error),
            done => return done,
        }
    }
    Err(failure.unwrap_or_else(|| CiError::new("no attempt was made")))
}

pub fn mint(
    app: &impl GitHubPost,
    owner: &str,
    repositories: &[String],
    pause: &dyn Fn(Duration),
) -> Result<Zeroizing<String>, CiError> {
    let mut bad = std::iter::once(owner)
        .filter(|name| !valid_owner(name))
        .chain(
            repositories
                .iter()
                .map(String::as_str)
                .filter(|name| !valid_repository(name)),
        );
    if let Some(bad) = bad.next() {
        return Err(CiError::new(format!(
            "{} is not a plain GitHub owner or repository name",
            crate::escape::inert(bad)
        )));
    }
    let first = repositories
        .first()
        .ok_or_else(|| CiError::new("name at least one --repository"))?;
    let lookup = format!("/repos/{owner}/{first}/installation");
    let installation: Installation = serde_json::from_str(&retried(pause, || app.get(&lookup))?)
        .map_err(|_| CiError::new("the App installation answer has no numeric id"))?;
    let path = format!("/app/installations/{}/access_tokens", installation.id);
    let request = token_request(repositories);
    let answer = Zeroizing::new(retried(pause, || app.post(&path, &request))?);
    let minted: Minted = serde_json::from_str(&answer)
        .map_err(|_| CiError::new("the installation token answer holds no token"))?;
    let token = Zeroizing::new(minted.token);
    if token.is_empty()
        || !token
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '-'))
    {
        return Err(CiError::new(format!(
            "the installation token answer holds a token that is not plain letters, digits, _, . and -"
        )));
    }
    Ok(token)
}
