// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

pub(super) fn is_injected_arg(name: &str) -> bool {
    matches!(
        name,
        "BUILDPLATFORM"
            | "TARGETPLATFORM"
            | "BUILDOS"
            | "BUILDARCH"
            | "BUILDVARIANT"
            | "TARGETOS"
            | "TARGETARCH"
            | "TARGETVARIANT"
    )
}

#[derive(Clone)]
pub(super) enum ArgValue {
    Unset,
    Known(String),
    Unknown,
}

pub(super) fn arg_value(value: &str) -> Option<String> {
    if value.contains('\\') {
        return None;
    }
    let value = if value.starts_with(['"', '\'']) {
        let quote = value.chars().next()?;
        value.strip_prefix(quote)?.strip_suffix(quote)?
    } else {
        if value.chars().any(char::is_whitespace) {
            return None;
        }
        value
    };
    if value.contains(['"', '\'']) {
        return None;
    }
    Some(value.to_owned())
}

pub(super) fn substitute(raw: &str, args: &BTreeMap<String, ArgValue>) -> Option<String> {
    let mut out = String::new();
    let mut rest = raw;
    while let Some(at) = rest.find('$') {
        out.push_str(&rest[..at]);
        let tail = &rest[at + 1..];
        let (name, default, consumed) = parse_variable(tail);
        if !valid_name(name) || consumed == 0 {
            return None;
        }
        if is_injected_arg(name) {
            return None;
        }
        let value = match args.get(name) {
            Some(ArgValue::Known(value)) if value.is_empty() && default.is_some() => default?,
            Some(ArgValue::Known(value)) => value,
            Some(ArgValue::Unknown) => return None,
            Some(ArgValue::Unset) | None => default?,
        };
        if value.contains(['$', '\\', '\'', '"']) {
            return None;
        }
        out.push_str(value);
        rest = &tail[consumed..];
    }
    out.push_str(rest);
    Some(out)
}

fn parse_variable(tail: &str) -> (&str, Option<&str>, usize) {
    if let Some(body) = tail.strip_prefix('{')
        && let Some(end) = body.find('}')
    {
        let inner = &body[..end];
        if inner.contains(['$', '{']) {
            return ("", None, 0);
        }
        let (name, default) = match inner.split_once(":-") {
            Some((name, default)) => (name, Some(default)),
            None => (inner, None),
        };
        return (name, default, end + 2);
    }
    let end = tail
        .bytes()
        .position(|b| !(b.is_ascii_alphanumeric() || b == b'_'))
        .unwrap_or(tail.len());
    (&tail[..end], None, end)
}

fn valid_name(name: &str) -> bool {
    let mut bytes = name.bytes();
    bytes
        .next()
        .is_some_and(|byte| byte.is_ascii_alphabetic() || byte == b'_')
        && bytes.all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
}
