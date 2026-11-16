// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use crate::cache::{KEEP_NEWEST, born_expired, should_save, trim};
use crate::error::CiError;
use crate::github::Repo;
use crate::logscan::scan;
use crate::time::date_text;

const BORN_FILE: &str = ".velnor/born";
const INCREMENTAL: &str = "incremental";
const MAX_AGE_VARIABLE: &str = "VELNOR_BASE_MAX_AGE_DAYS";
const REPOSITORY_VARIABLE: &str = "GITHUB_REPOSITORY";
const REF_VARIABLE: &str = "GITHUB_REF";

#[derive(Debug)]
pub struct Decision {
    pub save: bool,
    pub report: String,
}

pub fn decide(
    log: Option<&str>,
    target: &Path,
    archive: Option<&Path>,
    now_seconds: i64,
) -> Result<Decision, CiError> {
    let counts = log.map(scan).unwrap_or_default();
    decide_counts(&counts, target, archive, now_seconds)
}

pub(super) fn decide_counts(
    counts: &crate::logscan::LogScan,
    target: &Path,
    archive: Option<&Path>,
    now_seconds: i64,
) -> Result<Decision, CiError> {
    if !should_save(counts) {
        return Ok(Decision {
            save: false,
            report: "no crate was compiled, checked or documented in this run; saving nothing\n"
                .to_owned(),
        });
    }
    let removed = remove_incremental(target)?;
    if let Some(archive) = archive {
        remove_file(archive)?;
    }
    let born = target.join(BORN_FILE);
    if !born.is_file() {
        if let Some(parent) = born.parent() {
            fs::create_dir_all(parent)
                .map_err(|e| CiError::new(format!("cannot create {}: {e}", parent.display())))?;
        }
        fs::write(&born, format!("{}\n", date_text(now_seconds)))
            .map_err(|e| CiError::new(format!("cannot write {}: {e}", born.display())))?;
    }
    Ok(Decision {
        save: true,
        report: format!(
            "{} crates compiled, checked or documented; saving {}: {} bytes, removed {removed} incremental directories\n",
            counts.crates_compiled(),
            target.display(),
            directory_size(target)
        ),
    })
}

fn remove_file(path: &Path) -> Result<(), CiError> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == ErrorKind::NotFound => Ok(()),
        Err(e) => Err(CiError::new(format!(
            "cannot remove {}: {e}",
            path.display()
        ))),
    }
}

fn remove_incremental(target: &Path) -> Result<u64, CiError> {
    let Ok(entries) = fs::read_dir(target) else {
        return Ok(0);
    };
    let mut removed = 0;
    for entry in entries.flatten() {
        if !entry.file_type().is_ok_and(|kind| kind.is_dir()) {
            continue;
        }
        let path = entry.path();
        removed += remove_incremental_child(&path)?;
        let Ok(nested) = fs::read_dir(&path) else {
            continue;
        };
        for child in nested.flatten() {
            if child.file_type().is_ok_and(|kind| kind.is_dir()) {
                removed += remove_incremental_child(&child.path())?;
            }
        }
    }
    Ok(removed)
}

fn remove_incremental_child(dir: &Path) -> Result<u64, CiError> {
    let path = dir.join(INCREMENTAL);
    if !fs::symlink_metadata(&path).is_ok_and(|meta| meta.is_dir()) {
        return Ok(0);
    }
    fs::remove_dir_all(&path)
        .map_err(|e| CiError::new(format!("cannot remove {}: {e}", path.display())))?;
    Ok(1)
}

fn directory_size(dir: &Path) -> u64 {
    let Ok(entries) = fs::read_dir(dir) else {
        return 0;
    };
    entries
        .flatten()
        .map(|entry| match entry.file_type() {
            Ok(kind) if kind.is_dir() => directory_size(&entry.path()),
            Ok(kind) if kind.is_file() => entry.metadata().map_or(0, |meta| meta.len()),
            _ => 0,
        })
        .sum()
}

pub fn expire_old_bases(targets: &[PathBuf], now_seconds: i64) -> Result<String, CiError> {
    let Some(days) = std::env::var(MAX_AGE_VARIABLE)
        .ok()
        .and_then(|text| text.trim().parse::<u64>().ok())
        .filter(|days| *days > 0)
    else {
        return Ok(String::new());
    };
    let mut removed = Vec::new();
    for target in targets {
        let born = match fs::read_to_string(target.join(BORN_FILE)) {
            Ok(born) if born_expired(&born, now_seconds, days) => born.trim().to_owned(),
            Err(_) if target.is_dir() => "unknown".to_owned(),
            Ok(_) | Err(_) => continue,
        };
        fs::remove_dir_all(target)
            .map_err(|e| CiError::new(format!("cannot remove {}: {e}", target.display())))?;
        removed.push(format!("{} (born {born})", target.display()));
    }
    Ok(if removed.is_empty() {
        String::new()
    } else {
        format!(
            "removed build directories older than {days} days, so this run builds cold and saves a clean base: {}\n",
            removed.join(", ")
        )
    })
}

pub(super) fn run_trim(prefix: &str, keep: Option<usize>) -> Result<String, CiError> {
    let repository = std::env::var(REPOSITORY_VARIABLE)
        .map_err(|_| CiError::new(format!("set {REPOSITORY_VARIABLE} to owner/name")))?;
    let reference = std::env::var(REF_VARIABLE)
        .map_err(|_| CiError::new(format!("set {REF_VARIABLE} to the ref that saved")))?;
    let token = match super::token() {
        Ok(token) => token,
        Err(error) => return Ok(trim_warning(&error)),
    };
    let github =
        super::http::HttpGitHub::new(super::api_url(), token, super::http::DEFAULT_TIMEOUT);
    match trim(
        &github,
        &Repo::parse(&repository)?,
        &reference,
        prefix,
        keep.unwrap_or(KEEP_NEWEST),
    ) {
        Ok(report) => Ok(report),
        Err(error) => Ok(trim_warning(&error)),
    }
}

fn trim_warning(error: &CiError) -> String {
    format!("::warning::cache trim failed, older entries stay until the next save: {error}\n")
}
