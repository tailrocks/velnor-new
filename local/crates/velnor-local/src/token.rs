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
    let claims: Claims = signed_claims(token, public_key_der)?;
    if now < claims.nbf {
        return Err("the runtime token is not valid yet".to_owned());
    }
    if claims.exp <= now {
        return Err("the runtime token has expired".to_owned());
    }
    Ok(claims)
}

/// What runner.server signs into the `github.token` of a job that has `actions: write` on an
/// event that is not a pull request, and into no other job's token.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
pub struct JobGrant {
    pub velnor: String,
    pub repository: String,
    pub event: String,
    pub actions: String,
    pub exp: u64,
}

const GRANT_MARKER: &str = "job-cache-admin";

impl JobGrant {
    /// Whether the job may list and delete the caches of `repository`, as GitHub lets a job that
    /// has `actions: write`. A pull request, a fork's too, is untrusted whatever it carries.
    #[must_use]
    pub fn allows(&self, repository: &str) -> bool {
        self.velnor == GRANT_MARKER
            && self.actions == "write"
            && !self.event.starts_with("pull_request")
            && self.repository.eq_ignore_ascii_case(repository)
    }
}

/// The grant of a `github.token`, or why the token carries none. A token that is not signed by
/// runner.server's key, has expired or is no grant at all is an error, never a grant.
pub fn verify_job_grant(token: &str, public_key_der: &[u8], now: u64) -> Result<JobGrant, String> {
    let grant: JobGrant = signed_claims(token, public_key_der)?;
    if grant.exp <= now {
        return Err("the job token has expired".to_owned());
    }
    Ok(grant)
}

fn signed_claims<T: serde::de::DeserializeOwned>(
    token: &str,
    public_key_der: &[u8],
) -> Result<T, String> {
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
    serde_json::from_slice(&decode(payload)?)
        .map_err(|e| format!("the runtime token claims are invalid: {e}"))
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
