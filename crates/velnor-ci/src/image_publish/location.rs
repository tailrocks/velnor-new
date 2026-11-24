// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::digest::Digest;
use crate::error::CiError;

pub const REGISTRY: &str = "ghcr.io";
pub const DOWNLOAD_HOST: &str = "pkg-containers.githubusercontent.com";
pub const PACKAGE: &str = "tailrocks/velnor-job";
const MAX_LOCATION: usize = 4096;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Blobs,
    Manifests,
}

impl Kind {
    fn segment(self) -> &'static str {
        match self {
            Self::Blobs => "blobs",
            Self::Manifests => "manifests",
        }
    }
}

#[must_use]
pub fn object(kind: Kind, digest: &Digest) -> String {
    format!(
        "https://{REGISTRY}/v2/{PACKAGE}/{}/{}",
        kind.segment(),
        digest.as_str()
    )
}

#[must_use]
pub fn uploads() -> String {
    format!("https://{REGISTRY}/v2/{PACKAGE}/blobs/uploads/")
}

#[must_use]
pub fn api() -> String {
    format!("https://{REGISTRY}/v2/")
}

fn fail<T>(message: &str) -> Result<T, CiError> {
    Err(CiError::new(message))
}

fn unsafe_byte(byte: u8) -> bool {
    byte < 32 || byte == 127 || byte == b'\\'
}

fn decoded(text: &str) -> Vec<u8> {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        let pair = bytes
            .get(index + 1..index + 3)
            .and_then(|pair| std::str::from_utf8(pair).ok())
            .and_then(|pair| u8::from_str_radix(pair, 16).ok());
        match (bytes[index], pair) {
            (b'%', Some(value)) => {
                out.push(value);
                index += 3;
            }
            (byte, _) => {
                out.push(byte);
                index += 1;
            }
        }
    }
    out
}

fn dotted(path: &[u8]) -> bool {
    path.split(|byte| *byte == b'/')
        .any(|part| part == b"." || part == b"..")
}

struct Parts<'a> {
    path: &'a str,
    query: &'a str,
}

fn split<'a>(url: &'a str, host: &str) -> Result<Parts<'a>, CiError> {
    const OUTSIDE: &str = "Registry URL is outside the fixed HTTPS origin.";
    if url.len() > MAX_LOCATION || url.bytes().any(|byte| unsafe_byte(byte) || byte == b' ') {
        return fail(OUTSIDE);
    }
    let Some(rest) = url.strip_prefix("https://") else {
        return fail(OUTSIDE);
    };
    if rest.contains('#') {
        return fail(OUTSIDE);
    }
    let end = rest.find(['/', '?']).unwrap_or(rest.len());
    let (authority, tail) = rest.split_at(end);
    if authority != host && authority.strip_prefix(host) != Some(":443") {
        return fail(OUTSIDE);
    }
    let (path, query) = tail.split_once('?').unwrap_or((tail, ""));
    if !path.starts_with('/') {
        return fail(OUTSIDE);
    }
    Ok(Parts { path, query })
}

fn clean(parts: &Parts<'_>, message: &str) -> Result<(), CiError> {
    let path = decoded(parts.path);
    let query = decoded(parts.query);
    if path.iter().chain(&query).any(|byte| unsafe_byte(*byte)) || dotted(&path) {
        return fail(message);
    }
    if parts
        .query
        .split('&')
        .filter(|pair| !pair.is_empty())
        .any(|pair| pair.starts_with('='))
    {
        return fail("Registry upload query has an incorrect format.");
    }
    Ok(())
}

pub fn fixed(url: &str, host: &str) -> Result<(), CiError> {
    let parts = split(url, host)?;
    clean(&parts, "Registry URL has an incorrect character.")
}

pub fn upload(location: &str) -> Result<String, CiError> {
    const OUTSIDE: &str = "Registry upload location is outside the fixed HTTPS upload path.";
    if location.is_empty() || location.len() > MAX_LOCATION {
        return fail("Registry upload location is missing or too long.");
    }
    if dotted(&decoded(
        location.split(['?', '#']).next().unwrap_or_default(),
    )) {
        return fail(OUTSIDE);
    }
    let resolved = if location.starts_with("https://") {
        location.to_owned()
    } else if location.starts_with('/') && !location.starts_with("//") {
        format!("https://{REGISTRY}{location}")
    } else {
        return fail(OUTSIDE);
    };
    let parts = split(&resolved, REGISTRY).or_else(|_| fail(OUTSIDE))?;
    let path = decoded(parts.path);
    let prefixes = [
        format!("/v2/{PACKAGE}/blobs/upload/"),
        format!("/v2/{PACKAGE}/blobs/uploads/"),
    ];
    if !prefixes
        .iter()
        .any(|prefix| path.starts_with(prefix.as_bytes()))
    {
        return fail(OUTSIDE);
    }
    clean(&parts, OUTSIDE)?;
    Ok(resolved)
}

pub fn with_digest(location: &str, digest: &Digest) -> Result<String, CiError> {
    let (base, query) = location.split_once('?').unwrap_or((location, ""));
    if query
        .split('&')
        .any(|pair| pair.split('=').next() == Some("digest"))
    {
        return fail("Registry upload location contains a digest parameter.");
    }
    let encoded = format!("digest=sha256%3A{}", digest.hex());
    if query.is_empty() {
        Ok(format!("{base}?{encoded}"))
    } else {
        Ok(format!("{base}?{query}&{encoded}"))
    }
}

pub fn download(location: &str, digest: &Digest) -> Result<String, CiError> {
    const OUTSIDE: &str = "Registry download URL is outside the permitted HTTPS location.";
    if location.bytes().any(unsafe_byte) {
        return fail("Registry download URL has an incorrect character.");
    }
    let parts = split(location, DOWNLOAD_HOST).or_else(|_| fail(OUTSIDE))?;
    if !parts.path.ends_with(&format!("/blobs/{}", digest.as_str())) {
        return fail(OUTSIDE);
    }
    clean(&parts, "Registry download URL has an incorrect character.")?;
    Ok(location.to_owned())
}
