// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use ring::signature::{RSA_PKCS1_2048_8192_SHA256, UnparsedPublicKey};
use serde::Deserialize;
use velnor_cache_rules::{Access, Event, access_for};

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
pub struct Claims {
    pub repository: String,
    #[serde(rename = "ref")]
    pub git_ref: String,
    #[serde(rename = "defaultRef")]
    pub default_ref: String,
    pub nbf: u64,
    pub exp: u64,
    pub scp: String,
}

#[derive(Deserialize)]
struct Header {
    alg: String,
}

pub fn verify(token: &str, public_key_der: &[u8], now: u64) -> Result<Claims, String> {
    let mut parts = token.split('.');
    let (Some(header), Some(payload), Some(signature), None) =
        (parts.next(), parts.next(), parts.next(), parts.next())
    else {
        return Err("the runtime token is not a JWT".to_owned());
    };
    let decode = |part: &str| {
        URL_SAFE_NO_PAD
            .decode(part)
            .map_err(|e| format!("the runtime token is not base64url: {e}"))
    };
    let header: Header = serde_json::from_slice(&decode(header)?)
        .map_err(|e| format!("the runtime token header is invalid: {e}"))?;
    if header.alg != "RS256" {
        return Err(format!("the runtime token uses {}, not RS256", header.alg));
    }
    let signed = &token[..token.len() - signature.len() - 1];
    UnparsedPublicKey::new(&RSA_PKCS1_2048_8192_SHA256, public_key_der)
        .verify(signed.as_bytes(), &decode(signature)?)
        .map_err(|_| "the runtime token signature does not verify".to_owned())?;
    let claims: Claims = serde_json::from_slice(&decode(payload)?)
        .map_err(|e| format!("the runtime token claims are invalid: {e}"))?;
    if now < claims.nbf {
        return Err("the runtime token is not valid yet".to_owned());
    }
    if claims.exp <= now {
        return Err("the runtime token has expired".to_owned());
    }
    Ok(claims)
}

pub fn access(
    claims: &Claims,
    pull_request_bases: &BTreeMap<u64, String>,
) -> Result<Access, String> {
    let default_branch = claims
        .default_ref
        .strip_prefix("refs/heads/")
        .ok_or_else(|| format!("the default ref {} is not a branch", claims.default_ref))?;
    let event = event(&claims.git_ref, pull_request_bases)?;
    Ok(access_for(&event, default_branch))
}

fn event(git_ref: &str, pull_request_bases: &BTreeMap<u64, String>) -> Result<Event, String> {
    if let Some(branch) = git_ref.strip_prefix("refs/heads/") {
        return Ok(if branch.starts_with("gh-readonly-queue/") {
            Event::MergeGroup {
                queue_branch: branch.to_owned(),
            }
        } else {
            Event::Push {
                branch: branch.to_owned(),
            }
        });
    }
    if let Some(name) = git_ref.strip_prefix("refs/tags/") {
        return Ok(Event::Tag {
            name: name.to_owned(),
        });
    }
    let number = git_ref
        .strip_prefix("refs/pull/")
        .and_then(|rest| rest.strip_suffix("/merge"))
        .and_then(|number| number.parse::<u64>().ok())
        .ok_or_else(|| {
            format!("the ref {git_ref} is not a branch, tag or pull request merge ref")
        })?;
    let base_branch = pull_request_bases
        .get(&number)
        .ok_or_else(|| format!("no base branch is registered for pull request {number}"))?;
    Ok(Event::PullRequest {
        number,
        base_branch: base_branch.clone(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::engine::general_purpose::STANDARD;

    const NBF: u64 = 1_794_000_000;
    const EXP: u64 = 4_102_444_800;

    fn fixture() -> serde_json::Value {
        serde_json::from_str(include_str!("../tests/it/tokens.json"))
            .unwrap_or_else(|e| unreachable!("{e}"))
    }

    fn key() -> Vec<u8> {
        STANDARD
            .decode(fixture()["public_key"].as_str().unwrap_or_default())
            .unwrap_or_else(|e| unreachable!("{e}"))
    }

    fn token(name: &str) -> String {
        let fixture = fixture();
        let entry = &fixture["tokens"][name];
        format!(
            "{}.{}.{}",
            URL_SAFE_NO_PAD.encode(fixture["header"].as_str().unwrap_or_default()),
            URL_SAFE_NO_PAD.encode(entry["claims"].as_str().unwrap_or_default()),
            entry["signature"].as_str().unwrap_or_default()
        )
    }

    #[test]
    fn a_token_before_its_not_before_time_is_refused() {
        let refused = verify(&token("refs/heads/main"), &key(), NBF - 1);
        assert_eq!(
            refused,
            Err("the runtime token is not valid yet".to_owned())
        );
    }

    #[test]
    fn a_token_is_accepted_from_its_not_before_time_until_it_expires() {
        for now in [NBF, NBF + 1, EXP - 1] {
            let claims = verify(&token("refs/heads/main"), &key(), now);
            assert_eq!(claims.map(|c| c.nbf), Ok(NBF), "now = {now}");
        }
        assert_eq!(
            verify(&token("refs/heads/main"), &key(), EXP),
            Err("the runtime token has expired".to_owned())
        );
    }

    #[test]
    fn claims_without_a_numeric_not_before_are_malformed() {
        let base = r#""repository":"o/r","ref":"refs/heads/main","defaultRef":"refs/heads/main","exp":2,"scp":"s""#;
        for extra in ["", r#","nbf":"1""#] {
            let parsed = serde_json::from_str::<Claims>(&format!("{{{base}{extra}}}"));
            assert!(parsed.is_err(), "{extra}");
        }
    }
}
