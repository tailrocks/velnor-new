// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;
use std::fs;
use std::io::ErrorKind;
use std::path::{Component, Path, PathBuf};

use serde::{Deserialize, Serialize};
use velnor_rust::{INPUTS_LOCK, METADATA_PATH, sha256_hex};

use super::{input_scopes::Context, inputs};
use crate::error::CiError;
use crate::scope::Scope;

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Envelope {
    schema: u32,
    body: String,
    digest: String,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Cached {
    checkout: PathBuf,
    head: String,
    job: String,
    index: String,
    files: BTreeMap<PathBuf, Observation>,
    scopes: BTreeMap<PathBuf, Scope>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Observation {
    contents: bool,
    digest: String,
}

pub trait ScopeCacheIo {
    fn metadata(&self, path: &Path) -> Result<fs::Metadata, CiError> {
        fs::symlink_metadata(path).map_err(failed)
    }
    fn read_file(&self, path: &Path) -> Result<Option<Vec<u8>>, CiError> {
        match fs::read(path) {
            Ok(bytes) => Ok(Some(bytes)),
            Err(error) if error.kind() == ErrorKind::NotFound => Ok(None),
            Err(error) => Err(failed(error)),
        }
    }

    fn publish(&self, path: &Path, text: &str) -> Result<(), CiError> {
        inputs::publish_snapshot(path, text)
    }
}

pub(super) struct FileSystem;
impl ScopeCacheIo for FileSystem {}

pub(super) fn discard(path: &Path) -> Result<(), CiError> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(()),
        Err(error) => Err(failed(error)),
    }
}

pub(super) fn write(
    path: &Path,
    top: &Path,
    context: &Context,
    scopes: BTreeMap<PathBuf, Scope>,
    io: &dyn ScopeCacheIo,
) -> Result<(), CiError> {
    if context.index != index(top)? {
        return Err(failed("tracked index changed during detection"));
    }
    let tracked = inputs::tracked_paths(top)?;
    let mut watches: BTreeMap<PathBuf, bool> = tracked
        .paths
        .into_iter()
        .map(|path| (path, false))
        .collect();
    for path in context.files.keys().chain([&INPUTS_LOCK, &METADATA_PATH]) {
        watches.insert(PathBuf::from(path.as_str()), true);
    }
    for path in watches.keys().cloned().collect::<Vec<_>>() {
        for parent in path
            .ancestors()
            .skip(1)
            .filter(|path| !path.as_os_str().is_empty())
        {
            watches.entry(parent.to_path_buf()).or_insert(false);
        }
    }
    let consumed: BTreeMap<_, _> = context
        .files
        .iter()
        .map(|(path, bytes)| (PathBuf::from(path.as_str()), bytes.as_slice()))
        .collect();
    let files = watches
        .into_iter()
        .map(|(path, contents)| {
            let digest = if contents {
                consumed_fingerprint(&top.join(&path), consumed.get(&path).copied(), io)?
            } else {
                fingerprint(&top.join(&path), false)?
            };
            Ok((path, Observation { contents, digest }))
        })
        .collect::<Result<_, CiError>>()?;
    let cached = Cached {
        checkout: top.to_path_buf(),
        head: head(top)?,
        job: job(),
        index: context.index.clone(),
        files,
        scopes,
    };
    validate(&cached, top)?;
    let body = serde_json::to_string(&cached).map_err(failed)?;
    let envelope = Envelope {
        schema: 1,
        digest: sha256_hex(body.as_bytes()),
        body,
    };
    let text = serde_json::to_string(&envelope).map_err(failed)?;
    let published = io
        .publish(path, &text)
        .and_then(|()| validate(&cached, top));
    if let Err(error) = published {
        discard(path)?;
        return Err(error);
    }
    Ok(())
}

pub(super) fn read(path: &Path, root: &Path) -> Result<Scope, CiError> {
    if !fs::symlink_metadata(path).map_err(failed)?.is_file() {
        return Err(failed("cache is not a regular file"));
    }
    let envelope: Envelope =
        serde_json::from_slice(&fs::read(path).map_err(failed)?).map_err(failed)?;
    if envelope.schema != 1 || envelope.digest != sha256_hex(envelope.body.as_bytes()) {
        return Err(failed("cache schema or digest is invalid"));
    }
    let cached: Cached = serde_json::from_str(&envelope.body).map_err(failed)?;
    let top = inputs::tracked_paths(root)?.top;
    validate(&cached, &top)?;
    cached
        .scopes
        .get(root)
        .cloned()
        .ok_or_else(|| failed("cache has no scope for this root"))
}

fn validate(cached: &Cached, top: &Path) -> Result<(), CiError> {
    if cached.checkout != top
        || cached.head != head(top)?
        || cached.job != job()
        || cached.index != index(top)?
    {
        return Err(failed(
            "cache checkout, commit, job or tracked index changed",
        ));
    }
    for (path, observation) in &cached.files {
        if path.is_absolute()
            || path
                .components()
                .any(|component| !matches!(component, Component::Normal(_)))
        {
            return Err(failed("cache contains an invalid input path"));
        }
        if fingerprint(&top.join(path), observation.contents)? != observation.digest {
            return Err(failed(format!(
                "input {} changed after detection",
                path.display()
            )));
        }
    }
    Ok(())
}

fn consumed_fingerprint(
    path: &Path,
    bytes: Option<&[u8]>,
    io: &dyn ScopeCacheIo,
) -> Result<String, CiError> {
    let Some(bytes) = bytes else {
        return Ok("absent".to_owned());
    };
    let metadata = io.metadata(path)?;
    if !metadata.is_file() {
        return Err(failed("consumed input is no longer a regular file"));
    }
    Ok(sha256_hex(
        format!("file:{}:{}", mode(&metadata), sha256_hex(bytes)).as_bytes(),
    ))
}

fn fingerprint(path: &Path, contents: bool) -> Result<String, CiError> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok("absent".to_owned()),
        Err(error) => return Err(failed(error)),
    };
    let kind = if metadata.is_symlink() {
        format!(
            "link:{}",
            sha256_hex(
                fs::read_link(path)
                    .map_err(failed)?
                    .as_os_str()
                    .as_encoded_bytes()
            )
        )
    } else if metadata.is_file() {
        format!(
            "file:{}:{}",
            mode(&metadata),
            if contents {
                sha256_hex(&fs::read(path).map_err(failed)?)
            } else {
                String::new()
            }
        )
    } else if metadata.is_dir() {
        format!("directory:{}", mode(&metadata))
    } else {
        "other".to_owned()
    };
    Ok(sha256_hex(kind.as_bytes()))
}

#[cfg(unix)]
fn mode(metadata: &fs::Metadata) -> u32 {
    use std::os::unix::fs::PermissionsExt;
    metadata.permissions().mode()
}

#[cfg(not(unix))]
fn mode(metadata: &fs::Metadata) -> u32 {
    u32::from(metadata.permissions().readonly())
}

fn head(top: &Path) -> Result<String, CiError> {
    String::from_utf8(inputs::git(top, &["rev-parse", "--verify", "HEAD"], None)?).map_err(failed)
}

pub(super) fn index(top: &Path) -> Result<String, CiError> {
    Ok(sha256_hex(&inputs::git(
        top,
        &["ls-files", "-s", "-z", "--recurse-submodules"],
        None,
    )?))
}

fn job() -> String {
    std::env::var("GITHUB_JOB").unwrap_or_default()
}

fn failed(error: impl std::fmt::Display) -> CiError {
    CiError::new(format!("cannot reuse input scopes: {error}"))
}
