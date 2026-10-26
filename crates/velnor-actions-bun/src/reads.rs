// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde_json::Value;

use crate::jsonc;
use crate::paths::{is_tsconfig, is_under, normalize};

const SCRIPT_CONFIG_SUFFIXES: [&str; 6] = [
    ".config.ts",
    ".config.mts",
    ".config.cts",
    ".config.js",
    ".config.mjs",
    ".config.cjs",
];

#[must_use]
pub fn is_config_file(name: &str) -> bool {
    is_tsconfig(name)
        || SCRIPT_CONFIG_SUFFIXES
            .iter()
            .any(|suffix| name.ends_with(suffix))
}

#[must_use]
pub fn outside_reads(member_dir: &str, files: &[(String, String)]) -> Vec<String> {
    let mut reads = Vec::new();
    for (path, text) in files {
        let dir = path.rsplit_once('/').map_or("", |(dir, _)| dir);
        let name = path.rsplit('/').next().unwrap_or(path);
        let candidates = if is_tsconfig(name) {
            tsconfig_references(text).unwrap_or_else(|| vec![format!("<unparsable>/{path}")])
        } else if SCRIPT_CONFIG_SUFFIXES
            .iter()
            .any(|suffix| name.ends_with(suffix))
        {
            string_literals(text)
                .into_iter()
                .filter(|literal| is_relative_escape(literal))
                .collect()
        } else {
            Vec::new()
        };
        for candidate in candidates {
            if candidate.starts_with("<unparsable>/") {
                reads.push(candidate);
                continue;
            }
            match normalize(dir, &candidate) {
                None => reads.push(format!("<outside-repository>/{candidate}")),
                Some(resolved) if !is_under(&resolved, member_dir) => reads.push(resolved),
                Some(_) => {}
            }
        }
    }
    reads.sort();
    reads.dedup();
    reads
}

fn tsconfig_references(text: &str) -> Option<Vec<String>> {
    let root: Value = serde_json::from_str(&jsonc::to_json(text).ok()?).ok()?;
    let mut out = Vec::new();
    let mut take = |value: &Value| {
        if let Some(text) = value.as_str() {
            out.push(text.to_owned());
        }
    };
    match root.get("extends") {
        Some(Value::Array(items)) => items.iter().for_each(&mut take),
        Some(other) => take(other),
        None => {}
    }
    for reference in root
        .get("references")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        if let Some(path) = reference.get("path") {
            take(path);
        }
    }
    let options = root.get("compilerOptions");
    for (_, targets) in options
        .and_then(|options| options.get("paths"))
        .and_then(Value::as_object)
        .into_iter()
        .flatten()
    {
        targets.as_array().into_iter().flatten().for_each(&mut take);
    }
    for key in ["rootDirs", "typeRoots"] {
        options
            .and_then(|options| options.get(key))
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .for_each(&mut take);
    }
    for key in ["include", "exclude", "files"] {
        root.get(key)
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .for_each(&mut take);
    }
    out.retain(|candidate| is_relative_escape(candidate));
    Some(out)
}

fn is_relative_escape(candidate: &str) -> bool {
    candidate.split('/').any(|segment| segment == "..")
        || (candidate.starts_with("./") && candidate.contains("../"))
}

fn string_literals(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut chars = text.chars();
    while let Some(ch) = chars.next() {
        if !matches!(ch, '\'' | '"' | '`') {
            continue;
        }
        let mut literal = String::new();
        let mut closed = false;
        while let Some(next) = chars.next() {
            if next == '\\' {
                chars.next();
            } else if next == ch {
                closed = true;
                break;
            } else if next == '\n' && ch != '`' {
                break;
            } else {
                literal.push(next);
            }
        }
        if closed {
            out.push(literal);
        }
    }
    out
}
