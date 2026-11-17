// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_rust::package_token;

pub(super) enum Status {
    Ignore,
    Poison,
    Count { package: String, counter: bool },
}

#[must_use]
pub(super) fn status_line(line: &str) -> Status {
    let mut words = line.split_whitespace();
    let Some(first) = words.next() else {
        return Status::Ignore;
    };
    let (word, retried) = if first == "TRY" {
        let numbered = words
            .next()
            .is_some_and(|attempt| attempt.bytes().all(|byte| byte.is_ascii_digit()));
        match words.next() {
            Some(word) if numbered => (word, true),
            _ => return Status::Ignore,
        }
    } else {
        (first, false)
    };
    let Some((_, rest)) = line.split_once(word) else {
        return Status::Ignore;
    };
    let rest = rest.trim_start();
    if !rest.starts_with('[') {
        return Status::Ignore;
    }
    if matches!(word, "SKIP" | "SLOW") {
        return Status::Ignore;
    }
    let counted = matches!(
        word,
        "PASS" | "FAIL" | "TIMEOUT" | "TMPASS" | "LEAK" | "LEAK-FAIL" | "SIGSEGV" | "ABORT"
    );
    if !counted {
        return if word
            .bytes()
            .all(|byte| byte.is_ascii_uppercase() || byte == b'-')
        {
            Status::Poison
        } else {
            Status::Ignore
        };
    }
    if retried && word != "PASS" {
        return Status::Ignore;
    }
    parse_status(rest).map_or(Status::Poison, |(package, counter)| Status::Count {
        package,
        counter,
    })
}

fn parse_status(rest: &str) -> Option<(String, bool)> {
    let rest = rest.strip_prefix('[')?;
    let (inside, rest) = rest.split_once(']')?;
    if !duration_ok(inside) {
        return None;
    }
    let rest = rest.trim_start();
    let (rest, counter) = match rest.strip_prefix('(') {
        Some(counter) => (counter.split_once(')')?.1, true),
        None => (rest, false),
    };
    let binary = rest.split_whitespace().next()?;
    let package = binary.split("::").next()?;
    package_token(package).then(|| (package.to_owned(), counter))
}

fn duration_ok(inside: &str) -> bool {
    let Some(number) = inside.trim().strip_suffix('s') else {
        return false;
    };
    let mut parts = number.trim().split('.');
    let whole = parts.next().unwrap_or("");
    let fraction = parts.next();
    parts.next().is_none()
        && !whole.is_empty()
        && whole.bytes().all(|byte| byte.is_ascii_digit())
        && fraction.is_none_or(|fraction| {
            !fraction.is_empty() && fraction.bytes().all(|byte| byte.is_ascii_digit())
        })
}
