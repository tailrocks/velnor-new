// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use zeroize::Zeroizing;

use super::location::{PACKAGE, REGISTRY};
use crate::error::CiError;
use crate::image_candidate::json::parse;

const FORMAT: &str = "Registry authentication challenge has an incorrect format.";
const MAX_TOKEN: usize = 8192;
const MIN_TOKEN: usize = 16;

fn fail<T>(message: &str) -> Result<T, CiError> {
    Err(CiError::new(message))
}

fn parameter(text: &str) -> Option<(&str, &str, &str)> {
    let text = text.trim_start();
    let (name, rest) = text.split_once("=\"")?;
    let named = name.starts_with(|c: char| c.is_ascii_alphabetic())
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'));
    let (value, rest) = rest.split_once('"')?;
    if !named || value.contains('\\') {
        return None;
    }
    let rest = rest.trim_start();
    match rest.strip_prefix(',') {
        Some(after) => Some((name, value, after)),
        None if rest.is_empty() => Some((name, value, rest)),
        None => None,
    }
}

pub fn verify(header: &str) -> Result<(), CiError> {
    let Some(mut rest) = header
        .strip_prefix("Bearer")
        .filter(|rest| rest.starts_with(char::is_whitespace) && !rest.contains('\n'))
        .map(str::trim_start)
        .filter(|rest| !rest.is_empty())
    else {
        return fail(FORMAT);
    };
    let mut seen: Vec<(&str, &str)> = Vec::new();
    while !rest.is_empty() {
        let Some((name, value, after)) = parameter(rest) else {
            return fail(FORMAT);
        };
        if seen.iter().any(|(known, _)| *known == name) {
            return fail(FORMAT);
        }
        seen.push((name, value));
        rest = after;
    }
    let value = |name: &str| seen.iter().find(|(known, _)| *known == name).map(|p| p.1);
    let allowed = seen
        .iter()
        .all(|(name, _)| matches!(*name, "realm" | "service" | "scope"));
    if !allowed
        || value("realm") != Some(format!("https://{REGISTRY}/token").as_str())
        || value("service") != Some(REGISTRY)
    {
        return fail(FORMAT);
    }
    Ok(())
}

#[must_use]
pub fn token_url() -> String {
    format!(
        "https://{REGISTRY}/token?service={REGISTRY}&scope=repository%3A{}%3Apull%2Cpush",
        PACKAGE.replace('/', "%2F")
    )
}

#[must_use]
pub fn basic(actor: &str, secret: &str) -> Zeroizing<String> {
    let pair = Zeroizing::new(format!("{actor}:{secret}"));
    Zeroizing::new(format!("Basic {}", STANDARD.encode(pair.as_bytes())))
}

pub fn token(body: &[u8]) -> Result<Zeroizing<String>, CiError> {
    let value = parse(body, "Registry token response")?;
    let found = value
        .get("token")
        .or_else(|| value.get("access_token"))
        .and_then(serde_json::Value::as_str);
    let valid = found.filter(|text| {
        (MIN_TOKEN..=MAX_TOKEN).contains(&text.len())
            && text
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || b"._~=-".contains(&byte))
    });
    valid.map_or_else(
        || fail("Registry token response has no token in the required format."),
        |text| Ok(Zeroizing::new(text.to_owned())),
    )
}
