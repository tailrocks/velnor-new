// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;

use crate::logscan::{strip_ansi, strip_timestamp};

const MISSING: &str = "No such file or directory";
const SHOWN: usize = 10;
const EDGES: &[char] = &[
    '"', '\'', '`', ':', ';', ',', '(', ')', '[', ']', '{', '}', '<', '>',
];

struct Missing {
    path: String,
    package: Option<String>,
}

#[must_use]
pub fn hints(log: &str, top: &str, skipped: &BTreeSet<String>) -> Vec<String> {
    let found = missing(log, top, skipped);
    let mut hints: Vec<String> = found.iter().take(SHOWN).map(hint).collect();
    if found.len() > SHOWN {
        hints.push(format!(
            "{} more tracked files that sparse checkout left out were read and missing; add each one under [[inputs]] the same way",
            found.len() - SHOWN
        ));
    }
    hints
}

fn hint(missing: &Missing) -> String {
    let package = missing
        .package
        .as_deref()
        .unwrap_or("<the package whose test reads it>");
    format!(
        "Sparse checkout left out `{path}`, which a test read and did not find (No such file or directory). To check it out, add this to .velnor/rust-ci.toml and run `velnor-gen generate`:\n\n```toml\n[[inputs]]\nglob = \"{path}\"\npackages = [\"{package}\"]\n```\n",
        path = missing.path
    )
}

fn missing(log: &str, top: &str, skipped: &BTreeSet<String>) -> Vec<Missing> {
    let top = parts(top.trim_start_matches('/'));
    let mut seen = BTreeSet::new();
    let mut found = Vec::new();
    let mut package = None;
    for raw in log.lines() {
        let line = strip_ansi(strip_timestamp(raw));
        if let Some(id) = failing_test(&line) {
            package = Some(id);
        }
        if !line.contains(MISSING) {
            continue;
        }
        for token in line.split_whitespace() {
            let candidate = token.trim_matches(EDGES);
            let Some(path) = repo_path(candidate, &top).filter(|path| skipped.contains(path))
            else {
                continue;
            };
            if seen.insert(path.clone()) {
                found.push(Missing {
                    path,
                    package: package.clone(),
                });
            }
        }
    }
    found
}

fn failing_test(line: &str) -> Option<String> {
    let line = line.trim();
    let rest = if let Some((_, rest)) = line.split_once("STDERR:") {
        rest
    } else if line.starts_with("FAIL [") {
        let after = line.split_once(']')?.1.trim_start();
        match after.strip_prefix('(') {
            Some(counter) => counter.split_once(')')?.1,
            None => after,
        }
    } else {
        return None;
    };
    let binary = rest.split_whitespace().next()?;
    Some(binary.split("::").next()?.to_owned())
}

fn parts(path: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    for part in path.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            part => parts.push(part),
        }
    }
    parts
}

fn repo_path(token: &str, top: &[&str]) -> Option<String> {
    let parts = parts(token);
    let relative = if token.starts_with('/') {
        parts.strip_prefix(top)?
    } else {
        &parts[..]
    };
    (!relative.is_empty()).then(|| relative.join("/"))
}
