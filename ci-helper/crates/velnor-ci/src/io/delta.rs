// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::path::{Path, PathBuf};

use super::inputs::{modified_ns, set_modified_ns};
use crate::delta_cli::{DeltaArgs, DeltaCommand};
use crate::error::CiError;

const LIMIT: u64 = 500_000_000;
const MARKER: &str = ".velnor/delta-marker";
const INPUTS: &str = ".velnor/inputs.tsv";

pub(super) fn run(command: DeltaCommand) -> Result<String, CiError> {
    let base = std::env::var("BASE_CACHE_KEY").unwrap_or_default();
    match command {
        DeltaCommand::Prepare => {
            let hash = velnor_rust::sha256_hex(base.as_bytes());
            super::append_outputs(&format!("base8={}\n", &hash[..8]))?;
            Ok(format!("delta base: {base}\n"))
        }
        DeltaCommand::Apply(args) => apply(&args, &base),
        DeltaCommand::Stage(args) => stage(&args, &base),
    }
}

fn apply(args: &DeltaArgs, base: &str) -> Result<String, CiError> {
    safe_path(&args.target_dir)?;
    safe_path(&args.staging_dir)?;
    let restored = args.staging_dir.join("files");
    let identity_path = args.staging_dir.join("base-key");
    let identity = read_identity(&identity_path);
    let report = if !base.is_empty() && identity.as_deref() == Some(base) {
        match copy_tree(&restored, &args.target_dir) {
            Ok(count) => format!("applied {count} delta files, preserving modification times\n"),
            Err(error) => {
                remove(&args.staging_dir)?;
                super::cold::cold_on_failure(
                    std::slice::from_ref(&args.target_dir),
                    "delta apply",
                    || Err(error),
                )?
            }
        }
    } else {
        remove(&args.staging_dir)?;
        "no matching delta; using the base build directory\n".to_owned()
    };
    let marker = args.target_dir.join(MARKER);
    safe_path(&marker)?;
    create_parent(&marker)?;
    fs::write(&marker, "").map_err(|e| failure(&marker, &e))?;
    Ok(report)
}

fn read_identity(path: &Path) -> Option<String> {
    safe_path(path).ok()?;
    let metadata = fs::symlink_metadata(path).ok()?;
    if !metadata.is_file() || metadata.len() > 512 {
        return None;
    }
    let file = fs::File::open(path).ok()?;
    String::from_utf8(super::read_limited(file, 512).ok()?).ok()
}

fn stage(args: &DeltaArgs, base: &str) -> Result<String, CiError> {
    let log = std::env::var_os("RUNNER_TEMP")
        .and_then(|dir| fs::read_to_string(Path::new(&dir).join("velnor-run.log")).ok())
        .unwrap_or_default();
    let compiled = log.lines().any(|line| {
        let cleaned = crate::logscan::strip_ansi(crate::logscan::strip_timestamp(line));
        let mut words = cleaned.split_whitespace();
        matches!(words.next(), Some("Compiling" | "Checking" | "Documenting"))
            && words
                .next()
                .is_some_and(|name| args.packages.iter().any(|package| package == name))
    });
    if base.is_empty() || !compiled {
        super::append_outputs("save=false\n")?;
        return Ok("no base or no compilation; saving no delta\n".to_owned());
    }
    safe_path(&args.target_dir)?;
    safe_path(&args.staging_dir)?;
    let marker = modified_ns(&args.target_dir.join(MARKER))?;
    let payload = args.staging_dir.join("files");
    let prior = files(&payload)?;
    let mut selected = std::collections::BTreeSet::new();
    for path in files(&args.target_dir)? {
        if path == Path::new(INPUTS)
            || modified_ns(&args.target_dir.join(&path))? >= marker
            || prior.contains(&path)
        {
            selected.insert(path);
        }
    }
    let size = selected.iter().try_fold(base.len() as u64, |size, path| {
        fs::metadata(args.target_dir.join(path))
            .map(|metadata| size.saturating_add(metadata.len()))
            .map_err(|e| failure(path, &e))
    })?;
    if size > LIMIT {
        super::append_outputs("save=false\n")?;
        return Ok(format!(
            "::notice::delta has {size} bytes, above the {LIMIT} byte limit; saving nothing\n"
        ));
    }
    remove(&args.staging_dir)?;
    for path in &selected {
        copy(&args.target_dir.join(path), &payload.join(path))?;
    }
    let identity = args.staging_dir.join("base-key");
    create_parent(&identity)?;
    fs::write(&identity, base).map_err(|e| failure(&identity, &e))?;
    super::append_outputs("save=true\n")?;
    Ok(format!(
        "staged {} cumulative delta files, {size} bytes\n",
        selected.len()
    ))
}

fn files(root: &Path) -> Result<std::collections::BTreeSet<PathBuf>, CiError> {
    safe_path(root)?;
    let mut result = std::collections::BTreeSet::new();
    if !root.exists() {
        return Ok(result);
    }
    let mut pending = vec![PathBuf::new()];
    while let Some(relative) = pending.pop() {
        let dir = root.join(&relative);
        for entry in fs::read_dir(&dir).map_err(|e| failure(&dir, &e))? {
            let entry = entry.map_err(|e| failure(&dir, &e))?;
            let path = relative.join(entry.file_name());
            if entry.file_name() == "incremental"
                || (path.starts_with(".velnor")
                    && path != Path::new(".velnor")
                    && path != Path::new(INPUTS))
            {
                continue;
            }
            let kind = entry.file_type().map_err(|e| failure(&entry.path(), &e))?;
            if kind.is_dir() {
                pending.push(path);
            } else if kind.is_file() {
                result.insert(path);
            } else {
                return Err(CiError::new(format!(
                    "{} is not a regular delta file or directory",
                    entry.path().display()
                )));
            }
        }
    }
    Ok(result)
}

fn copy_tree(from: &Path, to: &Path) -> Result<usize, CiError> {
    let listed = files(from)?;
    for path in &listed {
        copy(&from.join(path), &to.join(path))?;
    }
    Ok(listed.len())
}

fn copy(from: &Path, to: &Path) -> Result<(), CiError> {
    safe_path(from)?;
    safe_path(to)?;
    create_parent(to)?;
    fs::copy(from, to).map_err(|e| failure(to, &e))?;
    set_modified_ns(to, modified_ns(from)?)
}

fn safe_path(path: &Path) -> Result<(), CiError> {
    let mut current = PathBuf::new();
    for component in path.components() {
        current.push(component);
        match fs::symlink_metadata(&current) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                return Err(CiError::new(format!("{} is a symlink", current.display())));
            }
            Ok(_) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(failure(&current, &e)),
        }
    }
    Ok(())
}

fn create_parent(path: &Path) -> Result<(), CiError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| failure(parent, &e))?;
    }
    Ok(())
}

fn remove(path: &Path) -> Result<(), CiError> {
    match fs::remove_dir_all(path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(failure(path, &e)),
    }
}

fn failure(path: &Path, error: &std::io::Error) -> CiError {
    CiError::new(format!("{}: {error}", path.display()))
}
