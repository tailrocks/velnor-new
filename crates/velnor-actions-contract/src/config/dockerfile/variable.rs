// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

pub(super) fn is_injected_arg(name: &str) -> bool {
    matches!(
        name,
        "SOURCE_SHA"
            | "VELNOR_SOURCE_SHA"
            | "BUILDPLATFORM"
            | "TARGETPLATFORM"
            | "BUILDOS"
            | "BUILDARCH"
            | "BUILDVARIANT"
            | "TARGETOS"
            | "TARGETARCH"
            | "TARGETVARIANT"
    )
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

pub(super) fn substitute(raw: &str, args: &BTreeMap<String, Option<String>>) -> Option<String> {
    let mut out = String::new();
    let mut rest = raw;
    while let Some(at) = rest.find('$') {
        out.push_str(&rest[..at]);
        let tail = &rest[at + 1..];
        let (name, default, consumed) = parse_variable(tail);
        if name.is_empty() || consumed == 0 {
            return None;
        }
        if is_injected_arg(name) {
            return None;
        }
        let value = match args.get(name) {
            Some(Some(value)) if value.is_empty() && default.is_some() => default?,
            Some(Some(value)) => value,
            Some(None) => return None,
            None => default?,
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
