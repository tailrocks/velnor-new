// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;
use std::fs;
use std::io::ErrorKind;
use std::path::{Component, Path};

use super::http::{DEFAULT_TIMEOUT, HttpGitHub};
use crate::cli::{CheckConfigArgs, PublishArgs, RecordArgs};
use crate::error::CiError;
use crate::github::Repo;
use crate::publish::{Request, SUMS, publish};
use crate::release::{CargoPathEntry, PlainName, check_cargo_discovery, check_config, record};

pub(super) fn run_record(args: &RecordArgs) -> Result<String, CiError> {
    let outputs = args
        .outputs
        .iter()
        .map(|path| {
            fs::read_to_string(path)
                .map(|text| (path.display().to_string(), text))
                .map_err(|e| CiError::new(format!("cannot read {}: {e}", path.display())))
        })
        .collect::<Result<Vec<_>, CiError>>()?;
    let lines = record(&outputs)?;
    super::append_outputs(&lines)?;
    Ok(lines)
}

pub(super) fn run_check_config(args: &CheckConfigArgs) -> Result<String, CiError> {
    let files = args
        .files
        .iter()
        .map(|path| {
            fs::read_to_string(path)
                .map(|text| (path.display().to_string(), text))
                .map_err(|e| CiError::new(format!("cannot read {}: {e}", path.display())))
        })
        .collect::<Result<Vec<_>, CiError>>()?;
    let top = std::env::current_dir()
        .map_err(|e| CiError::new(format!("cannot read the working folder: {e}")))?;
    for path in &args.files {
        check_cargo_discovery(&discovery(&top, path)?)?;
    }
    check_config(&files)
}

const CONFIG_ENTRIES: [&str; 3] = [".cargo", ".cargo/config", ".cargo/config.toml"];

fn discovery(top: &Path, file: &Path) -> Result<Vec<CargoPathEntry>, CiError> {
    let relative = file.strip_prefix(top).unwrap_or(file);
    let root = relative.parent().unwrap_or_else(|| Path::new(""));
    if root.is_absolute() || root.components().any(|part| part == Component::ParentDir) {
        return Err(CiError::new(format!(
            "{} is not inside the checkout",
            file.display()
        )));
    }
    let mut entries = Vec::new();
    for directory in root.ancestors() {
        let here = top.join(directory);
        if !directory.as_os_str().is_empty() {
            entries.extend(entry(directory, &here, false)?);
        }
        for name in CONFIG_ENTRIES {
            entries.extend(entry(&directory.join(name), &here.join(name), true)?);
        }
    }
    Ok(entries)
}

fn entry(shown: &Path, real: &Path, with_text: bool) -> Result<Option<CargoPathEntry>, CiError> {
    let fail = |e: std::io::Error| CiError::new(format!("cannot inspect {}: {e}", shown.display()));
    let meta = match fs::symlink_metadata(real) {
        Ok(meta) => meta,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(fail(error)),
    };
    let symlink = meta.file_type().is_symlink();
    let text = if with_text && meta.is_file() {
        Some(fs::read_to_string(real).map_err(fail)?)
    } else {
        None
    };
    Ok((symlink || text.is_some()).then(|| CargoPathEntry {
        path: shown.display().to_string(),
        symlink,
        text,
    }))
}

pub(super) fn run_publish(args: &PublishArgs) -> Result<String, CiError> {
    let token = super::token()?;
    let repo = Repo::parse(&args.repo)?;
    let mut files = BTreeMap::new();
    for name in args.files.iter().map(PlainName::as_str).chain([SUMS]) {
        let path = args.dir.join(name);
        match fs::read(&path) {
            Ok(bytes) => {
                files.insert(name.to_owned(), bytes);
            }
            Err(error) if error.kind() == ErrorKind::NotFound => {}
            Err(error) => {
                return Err(CiError::new(format!(
                    "cannot read {}: {error}",
                    path.display()
                )));
            }
        }
    }
    let request = Request {
        tag: args.tag.clone(),
        app: args.app.clone(),
        names: args.files.clone(),
        files,
    };
    publish(
        &HttpGitHub::new(super::api_url(), token, DEFAULT_TIMEOUT),
        &repo,
        &request,
    )
}
