// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::{
    collections::BTreeMap,
    ffi::OsString,
    io::{BufRead, BufReader, Read, Seek, Write},
    path::Path,
};

use sha2::{Digest, Sha256};
use velnor_actions_mise::{CheckDeadline, IsolatedCommand};

use super::{source, validate};
use crate::{OrchestratorError, internal::internal};

const FILE_LIMIT: usize = 8 * 1024 * 1024;
const HEADER_LIMIT: usize = 128;

pub(super) fn read(
    root: &Path,
    commit: &str,
    paths: &[&str],
    deadline: CheckDeadline,
) -> Result<BTreeMap<String, String>, OrchestratorError> {
    if !source::is_sha1(commit) || paths.is_empty() {
        return Err(internal("runner_image_source_path"));
    }
    let mut input = tempfile::tempfile().map_err(|_| internal("runner_image_source_git"))?;
    for path in paths {
        if path.starts_with('/')
            || path.bytes().any(|byte| byte.is_ascii_control())
            || path
                .split('/')
                .any(|part| part.is_empty() || part == "." || part == "..")
        {
            return Err(internal("runner_image_source_path"));
        }
        writeln!(input, "{commit}:{path}").map_err(|_| internal("runner_image_source_git"))?;
    }
    input
        .rewind()
        .map_err(|_| internal("runner_image_source_git"))?;
    let mut metadata = tempfile::tempfile().map_err(|_| internal("runner_image_source_git"))?;
    let metadata_cap = paths
        .len()
        .checked_mul(HEADER_LIMIT)
        .and_then(|value| u64::try_from(value).ok())
        .ok_or_else(|| internal("runner_image_source_file"))?;
    run_git(
        root,
        input
            .try_clone()
            .map_err(|_| internal("runner_image_source_git"))?,
        &mut metadata,
        "--batch-check",
        metadata_cap,
        deadline,
    )?;
    metadata
        .rewind()
        .map_err(|_| internal("runner_image_source_file"))?;
    let mut reader = BufReader::new(metadata);
    let mut headers = Vec::with_capacity(paths.len());
    let mut cap = 0_u64;
    for _ in paths {
        deadline
            .remaining()
            .map_err(|_| internal("runner_image_source_git"))?;
        let header = read_header(&mut reader)?;
        cap = cap
            .checked_add(header.1 as u64 + HEADER_LIMIT as u64 + 1)
            .ok_or_else(|| internal("runner_image_source_file"))?;
        headers.push(header);
    }
    finish(&mut reader)?;
    input
        .rewind()
        .map_err(|_| internal("runner_image_source_git"))?;
    let mut output = tempfile::tempfile().map_err(|_| internal("runner_image_source_git"))?;
    run_git(root, input, &mut output, "--batch", cap, deadline)?;
    output
        .rewind()
        .map_err(|_| internal("runner_image_source_file"))?;
    let mut reader = BufReader::new(output);
    let mut result = BTreeMap::new();
    for (path, expected) in paths.iter().zip(headers) {
        deadline
            .remaining()
            .map_err(|_| internal("runner_image_source_git"))?;
        let (actual, committed) = read_blob(&mut reader)?;
        admit_header(&expected, &actual)?;
        let working = validate::read_limited(&root.join(path), FILE_LIMIT as u64)?;
        if result
            .insert((*path).to_owned(), compare_digest(&committed, &working)?)
            .is_some()
        {
            return Err(internal("runner_image_source_path"));
        }
    }
    finish(&mut reader)?;
    Ok(result)
}

fn admit_header(
    expected: &(String, usize),
    actual: &(String, usize),
) -> Result<(), OrchestratorError> {
    if actual != expected {
        return Err(internal("runner_image_source_file"));
    }
    Ok(())
}

fn compare_digest(committed: &[u8], working: &[u8]) -> Result<String, OrchestratorError> {
    if committed != working {
        return Err(internal("runner_builder_source_mismatch"));
    }
    Ok(source::hex(&Sha256::digest(committed)))
}

fn run_git(
    root: &Path,
    input: std::fs::File,
    output: &mut std::fs::File,
    mode: &str,
    cap: u64,
    deadline: CheckDeadline,
) -> Result<(), OrchestratorError> {
    let result = IsolatedCommand::repo_task(
        "git",
        vec![OsString::from("cat-file"), OsString::from(mode)],
        &[
            (
                OsString::from("GIT_NO_REPLACE_OBJECTS"),
                OsString::from("1"),
            ),
            (OsString::from("GIT_OPTIONAL_LOCKS"), OsString::from("0")),
        ],
    )
    .map_err(|_| internal("runner_image_source_git"))?
    .with_cwd(root.to_path_buf())
    .run_files_until(input, output, cap, 8 * 1024 * 1024, deadline)
    .map_err(|_| internal("runner_image_source_git"))?;
    if !result.success {
        return Err(internal("runner_image_source_file"));
    }
    Ok(())
}

fn finish(reader: &mut impl Read) -> Result<(), OrchestratorError> {
    let mut trailing = [0];
    if reader
        .read(&mut trailing)
        .map_err(|_| internal("runner_image_source_file"))?
        != 0
    {
        return Err(internal("runner_image_source_file"));
    }
    Ok(())
}

fn read_header(reader: &mut impl BufRead) -> Result<(String, usize), OrchestratorError> {
    let mut header = Vec::new();
    reader
        .by_ref()
        .take(HEADER_LIMIT as u64)
        .read_until(b'\n', &mut header)
        .map_err(|_| internal("runner_image_source_file"))?;
    let header = std::str::from_utf8(&header).map_err(|_| internal("runner_image_source_file"))?;
    let header = header
        .strip_suffix('\n')
        .ok_or_else(|| internal("runner_image_source_file"))?;
    let mut fields = header.split(' ');
    let oid = fields
        .next()
        .ok_or_else(|| internal("runner_image_source_file"))?;
    let kind = fields
        .next()
        .ok_or_else(|| internal("runner_image_source_file"))?;
    let size = fields
        .next()
        .ok_or_else(|| internal("runner_image_source_file"))?;
    if !source::is_sha1(oid)
        || kind != "blob"
        || fields.next().is_some()
        || size.is_empty()
        || !size.bytes().all(|byte| byte.is_ascii_digit())
    {
        return Err(internal("runner_image_source_file"));
    }
    let size: usize = size
        .parse()
        .map_err(|_| internal("runner_image_source_file"))?;
    if size > FILE_LIMIT {
        return Err(internal("runner_image_source_file"));
    }
    Ok((oid.to_owned(), size))
}

fn read_blob(reader: &mut impl BufRead) -> Result<((String, usize), Vec<u8>), OrchestratorError> {
    let header = read_header(reader)?;
    let mut committed = vec![0; header.1];
    reader
        .read_exact(&mut committed)
        .map_err(|_| internal("runner_image_source_file"))?;
    let mut separator = [0];
    reader
        .read_exact(&mut separator)
        .map_err(|_| internal("runner_image_source_file"))?;
    if separator != *b"\n" {
        return Err(internal("runner_image_source_file"));
    }
    Ok((header, committed))
}

#[cfg(test)]
#[path = "runner_source_batch_tests.rs"]
mod tests;
