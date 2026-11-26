// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0
// SPDX-FileContributor: tailrocks/velnor-legacy@1b9fc0388925d21ff44ba1be5027684c38c8e4c9 crates/velnor-runner/src/executor.rs

use super::error::ExprError;
use super::glob::{OrderedGlobs, collect_files, search_roots};
use super::value::Value;
use ring::digest::{Context, Digest, SHA256};
use std::collections::HashSet;
use std::fmt::Write as _;
use std::fs::File;
use std::io::{self, Read};
use std::path::Path;

const READ_BUFFER_BYTES: usize = 64 * 1024;

pub(crate) fn hash_files(workspace: &Path, args: &[Value]) -> Result<String, ExprError> {
    let patterns: Vec<String> = args.iter().map(Value::convert_to_string).collect();
    if let Some(option) = patterns.first().filter(|first| first.starts_with("--")) {
        return Err(ExprError::evaluation(format!(
            "Invalid glob option {option}"
        )));
    }
    let failure = |reason: &str| {
        ExprError::evaluation(format!(
            "hashFiles('{}') failed. {reason}",
            patterns.join(", ")
        ))
    };
    let globs = OrderedGlobs::new(&patterns).map_err(|_| failure("Invalid glob pattern"))?;

    let mut seen = HashSet::new();
    let mut matches = Vec::new();
    for root in search_roots(workspace, &patterns) {
        for path in collect_files(&root) {
            let Ok(relative) = path.strip_prefix(workspace) else {
                continue;
            };
            if globs.is_match(&normalize_path(relative)) && seen.insert(path.clone()) {
                matches.push(path);
            }
        }
    }
    if matches.is_empty() {
        return Ok(String::new());
    }

    let mut buffer = vec![0_u8; READ_BUFFER_BYTES];
    let mut aggregate = Context::new(&SHA256);
    for path in &matches {
        let digest = file_digest(path, &mut buffer).map_err(|_| {
            failure(&format!(
                "Fail to hash files under directory '{}'",
                workspace.display()
            ))
        })?;
        aggregate.update(digest.as_ref());
    }
    Ok(hex_digest(aggregate.finish().as_ref()))
}

fn file_digest(path: &Path, buffer: &mut [u8]) -> io::Result<Digest> {
    let mut file = File::open(path)?;
    let mut context = Context::new(&SHA256);
    loop {
        let read = file.read(buffer)?;
        if read == 0 {
            return Ok(context.finish());
        }
        context.update(&buffer[..read]);
    }
}

fn normalize_path(path: &Path) -> String {
    path.components()
        .map(|component| component.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/")
}

fn hex_digest(bytes: &[u8]) -> String {
    bytes
        .iter()
        .fold(String::with_capacity(bytes.len() * 2), |mut out, byte| {
            let _ = write!(out, "{byte:02x}");
            out
        })
}
