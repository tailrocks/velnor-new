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
use crate::release::{
    CargoPathEntry, PlainName, check_cargo_discovery, check_config, check_manifests,
    check_tag_patterns, publishes_to_crates_io, record,
};
use crate::rulesets::check_tag_rulesets;

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
    let mut discovered = Vec::new();
    for path in &args.files {
        let entries = discovery(&top, path)?;
        check_cargo_discovery(&entries, false)?;
        discovered.push(entries);
    }
    let output = check_config(&files)?;
    let known = if args.manifests {
        let roots: Vec<String> = args.files.iter().map(|path| root_of(&top, path)).collect();
        let known = super::manifests::read(&top, &roots)?;
        let relative: Vec<(String, String)> = args
            .files
            .iter()
            .zip(&files)
            .map(|(path, (_, text))| (relative_to(&top, path), text.clone()))
            .collect();
        check_manifests(
            &relative,
            &known.metadata,
            &known.manifests,
            &known.versions,
        )?;
        Some(known)
    } else {
        None
    };
    let metadata = known.as_ref().map(|known| &known.metadata);
    for ((path, (_, text)), entries) in args.files.iter().zip(&files).zip(&discovered) {
        let publishes = publishes_to_crates_io(&root_of(&top, path), text, metadata)?;
        check_cargo_discovery(entries, publishes)?;
    }
    let patterns = args.tag_patterns();
    check_tag_patterns(&files, &patterns)?;
    if let Some(repo) = &args.repo {
        check_tag_rulesets(
            &HttpGitHub::new(super::api_url(), super::token()?, DEFAULT_TIMEOUT),
            &Repo::parse(repo)?,
            &patterns,
        )?;
    }
    Ok(output)
}

fn relative_to(top: &Path, file: &Path) -> String {
    file.strip_prefix(top)
        .unwrap_or(file)
        .components()
        .filter(|part| !matches!(part, Component::CurDir))
        .collect::<std::path::PathBuf>()
        .display()
        .to_string()
}

fn root_of(top: &Path, file: &Path) -> String {
    Path::new(&relative_to(top, file))
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .map_or_else(|| ".".to_owned(), |parent| parent.display().to_string())
}

const CONFIG_ENTRIES: [(&str, bool); 3] = [
    (".cargo", false),
    (".cargo/config", true),
    (".cargo/config.toml", true),
];

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
        for (name, with_text) in CONFIG_ENTRIES {
            entries.extend(entry(&directory.join(name), &here.join(name), with_text)?);
        }
    }
    Ok(entries)
}

fn entry(shown: &Path, real: &Path, with_text: bool) -> Result<Option<CargoPathEntry>, CiError> {
    let fail = |e: std::io::Error| CiError::new(format!("cannot inspect {}: {e}", shown.display()));
    let meta = match fs::symlink_metadata(real) {
        Ok(meta) => meta,
        Err(error) if matches!(error.kind(), ErrorKind::NotFound | ErrorKind::NotADirectory) => {
            return Ok(None);
        }
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
