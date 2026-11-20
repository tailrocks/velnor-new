// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};

use velnor_rust::sha256_hex;

use super::{Request, SUMS};
use crate::error::CiError;
use crate::release::PlainName;

pub(super) struct File<'a> {
    pub(super) name: &'a str,
    pub(super) sha256: String,
    pub(super) bytes: &'a [u8],
}

pub(super) fn verify(request: &Request) -> Result<(Vec<File<'_>>, Vec<String>), CiError> {
    if request.assetless {
        return if request.names.is_empty() && request.optional.is_empty() {
            Ok((Vec::new(), Vec::new()))
        } else {
            Err(CiError::new("an assetless release names no files"))
        };
    }
    let mut names = BTreeSet::new();
    for name in request.names.iter().chain(&request.optional) {
        if name.as_str() == SUMS {
            return Err(CiError::new(format!(
                "{SUMS} is published with every release; do not name it as a file"
            )));
        }
        if !names.insert(name.as_str()) {
            return Err(CiError::new(format!("file {name} is named twice")));
        }
    }
    let mut skipped = Vec::new();
    for name in &request.optional {
        if !request.files.contains_key(name.as_str()) {
            names.remove(name.as_str());
            skipped.push(format!("skipped optional {name}: not found"));
        }
    }
    if request.names.is_empty() {
        return Err(CiError::new("name at least one file to publish"));
    }
    let sums_bytes = request
        .files
        .get(SUMS)
        .ok_or_else(|| CiError::new(format!("{SUMS}: missing")))?;
    let sums = read_sums(sums_bytes)?;
    let mut problems = Vec::new();
    let mut files = Vec::new();
    for name in &names {
        let Some(bytes) = request.files.get(*name) else {
            problems.push(format!("{name}: missing"));
            continue;
        };
        let actual = sha256_hex(bytes);
        match sums.get(*name) {
            None => problems.push(format!("{name}: not listed in {SUMS}")),
            Some(listed) if *listed != actual => problems.push(format!(
                "{name}: FAILED, its SHA-256 is {actual} and {SUMS} lists {listed}"
            )),
            Some(_) => files.push(File {
                name,
                sha256: actual,
                bytes,
            }),
        }
    }
    problems.extend(
        sums.keys()
            .filter(|listed| !names.contains(listed.as_str()))
            .map(|listed| format!("{listed}: listed in {SUMS} and not a file of this release")),
    );
    if !problems.is_empty() {
        return Err(CiError::new(problems.join("\n")));
    }
    files.push(File {
        name: SUMS,
        sha256: sha256_hex(sums_bytes),
        bytes: sums_bytes,
    });
    Ok((files, skipped))
}

pub(crate) fn read_sums(bytes: &[u8]) -> Result<BTreeMap<String, String>, CiError> {
    let text =
        std::str::from_utf8(bytes).map_err(|_| CiError::new(format!("{SUMS} is not UTF-8")))?;
    let body = text
        .strip_suffix('\n')
        .ok_or_else(|| CiError::new(format!("{SUMS} is empty or does not end with a newline")))?;
    let mut sums = BTreeMap::new();
    for (index, line) in body.split('\n').enumerate() {
        let (sha256, name) = sum_line(line).ok_or_else(|| {
            CiError::new(format!(
                "line {} of {SUMS} is not a lowercase hex SHA-256, two spaces or a space and '*', and a file name",
                index + 1
            ))
        })?;
        if sums.insert(name.to_owned(), sha256.to_owned()).is_some() {
            return Err(CiError::new(format!("{SUMS} lists {name} twice")));
        }
    }
    Ok(sums)
}

fn sum_line(line: &str) -> Option<(&str, &str)> {
    let (sha256, rest) = line.split_at_checked(64)?;
    let name = rest
        .strip_prefix("  ")
        .or_else(|| rest.strip_prefix(" *"))?;
    let hex = sha256
        .bytes()
        .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'));
    (hex && PlainName::new("file", name).is_ok()).then_some((sha256, name))
}
