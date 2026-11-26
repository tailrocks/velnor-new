// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0
// SPDX-FileContributor: tailrocks/velnor-legacy@1b9fc0388925d21ff44ba1be5027684c38c8e4c9 crates/velnor-runner/src/executor.rs

use super::error::ExprError;
use super::glob::PatternSet;
use super::pattern::trim_line;
use super::value::Value;
use ring::digest::{Context, Digest, SHA256};
use std::fmt::{Display, Write as _};
use std::fs::{self, File};
use std::io::{self, Read};
use std::path::Path;

const READ_BUFFER_BYTES: usize = 64 * 1024;

pub(crate) fn hash_files(workspace: &Path, args: &[Value]) -> Result<String, ExprError> {
    let arguments: Vec<String> = args.iter().map(Value::convert_to_string).collect();
    if let Some(option) = arguments.first().filter(|first| first.starts_with("--")) {
        return Err(ExprError::evaluation(format!(
            "Invalid glob option {option}"
        )));
    }
    let failure = |detail: &dyn Display| {
        ExprError::evaluation(format!(
            "hashFiles('{}') failed. Fail to hash files under directory '{}'. {detail}",
            arguments.join(", ").replace('\'', "''"),
            workspace.display()
        ))
    };
    let root = fs::canonicalize(workspace).map_err(|error| failure(&error))?;
    let lines = arguments
        .iter()
        .flat_map(|argument| argument.split('\n'))
        .map(trim_line)
        .filter(|line| !line.is_empty() && !line.starts_with('#'));
    let patterns = PatternSet::new(&root, lines).map_err(|error| failure(&error))?;

    let mut buffer = vec![0_u8; READ_BUFFER_BYTES];
    let mut aggregate = Context::new(&SHA256);
    let mut count = 0_usize;
    for found in patterns.walk(&root) {
        let path = found.map_err(|error| failure(&error))?;
        match file_digest(&path, &mut buffer) {
            Ok(digest) => {
                aggregate.update(digest.as_ref());
                count += 1;
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(failure(&format!("{}: {error}", path.display()))),
        }
    }
    if count == 0 {
        return Ok(String::new());
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

fn hex_digest(bytes: &[u8]) -> String {
    bytes
        .iter()
        .fold(String::with_capacity(bytes.len() * 2), |mut out, byte| {
            let _ = write!(out, "{byte:02x}");
            out
        })
}
