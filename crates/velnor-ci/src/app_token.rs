// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

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

#[must_use]
pub fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && !name.starts_with('.')
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
}

pub fn mint(
    app: &impl GitHubPost,
    owner: &str,
    repositories: &[String],
) -> Result<Zeroizing<String>, CiError> {
    let mut names = std::iter::once(owner).chain(repositories.iter().map(String::as_str));
    if let Some(bad) = names.find(|name| !valid_name(name)) {
        return Err(CiError::new(format!(
            "{} is not a plain GitHub owner or repository name",
            crate::escape::inert(bad)
        )));
    }
    let first = repositories
        .first()
        .ok_or_else(|| CiError::new("name at least one --repository"))?;
    let installation: Installation =
        serde_json::from_str(&app.get(&format!("/repos/{owner}/{first}/installation"))?)
            .map_err(|_| CiError::new("the App installation answer has no numeric id"))?;
    let answer = Zeroizing::new(app.post(
        &format!("/app/installations/{}/access_tokens", installation.id),
        &token_request(repositories),
    )?);
    let minted: Minted = serde_json::from_str(&answer)
        .map_err(|_| CiError::new("the installation token answer holds no token"))?;
    let token = Zeroizing::new(minted.token);
    if token.is_empty() || !token.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
        return Err(CiError::new(
            "the installation token answer holds a token that is not plain letters, digits and _",
        ));
    }
    Ok(token)
}
