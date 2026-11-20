// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use super::cache_prune::prune;
use super::input_target::check_removal_target;
use crate::cache::{KEEP_NEWEST, born_expired, should_save, trim};
use crate::cache_prune::UsedUnits;
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
    let counts = scan(log.ok_or_else(|| CiError::new("velnor-run.log is missing"))?);
    decide_counts(&counts, target, archive, now_seconds, None, None)
}

pub(super) fn decide_counts(
    counts: &crate::logscan::LogScan,
    target: &Path,
    archive: Option<&Path>,
    now_seconds: i64,
    budget_bytes: Option<u64>,
    used: Option<&UsedUnits>,
) -> Result<Decision, CiError> {
    if !should_save(counts) {
        return Ok(Decision {
            save: false,
            report: "no crate was compiled, checked or documented in this run; saving nothing\n"
                .to_owned(),
        });
    }
    let before = directory_size(target)?;
    let removed = remove_incremental(target)?;
    if let Some(archive) = archive {
        remove_file(archive)?;
    }
    let pruned = prune(target, used)?;
    let size = directory_size(target)?;
    let basis = if used.is_some() {
        "this run's dependency units"
    } else {
        "no complete record of this run's units"
    };
    let pruning = format!(
        "pruned from {before} bytes to {size} bytes, keeping {basis}; removed {}",
        pruned.describe()
    );
    if let Some(budget) = budget_bytes.filter(|budget| size > *budget) {
        return Ok(Decision {
            save: false,
            report: format!(
                "::notice::{} is {size} bytes, more than the repository cache budget of {budget} bytes; saving nothing ({pruning})\n",
                target.display()
            ),
        });
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
            "{} crates compiled, checked or documented; saving {}: {size} bytes, removed {removed} incremental directories; {pruning}\n",
            counts.crates_compiled(),
            target.display(),
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

pub(super) fn entries(dir: &Path) -> Result<Vec<fs::DirEntry>, CiError> {
    fs::read_dir(dir)
        .and_then(Iterator::collect)
        .map_err(|e| CiError::new(format!("cannot traverse {}: {e}", dir.display())))
}

fn remove_incremental(target: &Path) -> Result<u64, CiError> {
    let mut removed = 0;
    for entry in entries(target)? {
        let kind = entry.file_type().map_err(|e| CiError::new(e.to_string()))?;
        if kind.is_symlink() {
            return Err(CiError::new("cache target contains a symlink"));
        }
        if !kind.is_dir() {
            continue;
        }
        let path = entry.path();
        removed += remove_incremental_child(&path)?;
        for child in entries(&path)? {
            let kind = child.file_type().map_err(|e| CiError::new(e.to_string()))?;
            if kind.is_symlink() {
                return Err(CiError::new("cache target contains a symlink"));
            }
            if kind.is_dir() {
                removed += remove_incremental_child(&child.path())?;
            }
        }
    }
    Ok(removed)
}

fn remove_incremental_child(dir: &Path) -> Result<u64, CiError> {
    let path = dir.join(INCREMENTAL);
    match fs::symlink_metadata(&path) {
        Err(e) if e.kind() == ErrorKind::NotFound => return Ok(0),
        Ok(meta) if meta.is_dir() => {}
        Ok(_) => {
            return Err(CiError::new(format!(
                "{} is not a regular directory",
                path.display()
            )));
        }
        Err(e) => return Err(CiError::new(e.to_string())),
    }
    fs::remove_dir_all(&path)
        .map_err(|e| CiError::new(format!("cannot remove {}: {e}", path.display())))?;
    Ok(1)
}

pub(super) fn directory_size(dir: &Path) -> Result<u64, CiError> {
    let mut size = 0;
    for entry in entries(dir)? {
        let kind = entry.file_type().map_err(|e| CiError::new(e.to_string()))?;
        if kind.is_symlink() {
            return Err(CiError::new("cache target contains a symlink"));
        }
        size += if kind.is_dir() {
            directory_size(&entry.path())?
        } else {
            entry
                .metadata()
                .map_err(|e| CiError::new(e.to_string()))?
                .len()
        };
    }
    Ok(size)
}

#[derive(Debug)]
pub struct Expiry {
    pub expired: bool,
    pub report: String,
}

pub fn expire_old_bases(targets: &[PathBuf], now_seconds: i64) -> Result<Expiry, CiError> {
    let Some(days) = std::env::var(MAX_AGE_VARIABLE)
        .ok()
        .and_then(|text| text.trim().parse::<u64>().ok())
        .filter(|days| *days > 0)
    else {
        return Ok(Expiry {
            expired: false,
            report: String::new(),
        });
    };
    let mut expired = Vec::new();
    for target in targets {
        let born = match fs::read_to_string(target.join(BORN_FILE)) {
            Ok(born) if born_expired(&born, now_seconds, days) => born.trim().to_owned(),
            Err(_) if target.is_dir() => "unknown".to_owned(),
            Ok(_) | Err(_) => continue,
        };
        expired.push((target, born));
    }
    for (target, _) in &expired {
        check_removal_target(target)?;
    }
    let mut removed = Vec::new();
    for (target, born) in expired {
        fs::remove_dir_all(target)
            .map_err(|e| CiError::new(format!("cannot remove {}: {e}", target.display())))?;
        removed.push(format!("{} (born {born})", target.display()));
    }
    Ok(Expiry {
        expired: !removed.is_empty(),
        report: if removed.is_empty() {
            String::new()
        } else {
            format!(
                "removed build directories older than {days} days, so this run builds cold and saves a clean base: {}\n",
                removed.join(", ")
            )
        },
    })
}

pub(super) fn run_trim(prefix: &str, keep: Option<usize>) -> Result<String, CiError> {
    let repository = std::env::var(REPOSITORY_VARIABLE)
        .map_err(|_| CiError::new(format!("set {REPOSITORY_VARIABLE} to owner/name")))?;
    let reference = std::env::var(REF_VARIABLE)
        .map_err(|_| CiError::new(format!("set {REF_VARIABLE} to the ref that saved")))?;
    let token = super::token()?;
    let github =
        super::http::HttpGitHub::new(super::api_url(), token, super::http::DEFAULT_TIMEOUT);
    trim(
        &github,
        &Repo::parse(&repository)?,
        &reference,
        prefix,
        keep.unwrap_or(KEEP_NEWEST),
        super::now_seconds(),
    )
}

fn budget_limit() -> Result<Option<u64>, CiError> {
    match std::env::var("VELNOR_CACHE_BUDGET_BYTES") {
        Ok(text) if !text.trim().is_empty() => text
            .trim()
            .parse()
            .map(Some)
            .map_err(|_| CiError::new("VELNOR_CACHE_BUDGET_BYTES is not a number of bytes")),
        _ => Ok(None),
    }
}

pub(super) fn run_decide(args: &crate::cli_cache::DecideArgs) -> Result<String, CiError> {
    let (target, archive) = super::inputs_batch::cache_paths(args)?;
    let root = super::current_dir()?;
    let target = super::cache_paths::validated(&root, &target)?;
    let temp = std::env::var_os("RUNNER_TEMP")
        .filter(|dir| !dir.is_empty())
        .ok_or_else(|| {
            CiError::new("set RUNNER_TEMP to the directory containing velnor-run.log")
        })?;
    let log_path = Path::new(&temp).join("velnor-run.log");
    let log = fs::read(&log_path)
        .map_err(|e| CiError::new(format!("cannot read {}: {e}", log_path.display())))?;
    let manifests = super::cache_paths::list("VELNOR_CACHE_MANIFESTS")?
        .unwrap_or_else(|| args.manifest_path.clone());
    let deadline = crate::public_probe::Deadline::after(std::time::Duration::from_secs(
        args.public_probe_seconds,
    ));
    let admission =
        super::cache_sources::refusal(&root, &manifests, &args.public_probe_api, deadline)?;
    let sources = admission.refusal.is_none();
    let (probe_log, mismatch) = (admission.log, admission.mismatch);
    let decision = if let Some(reason) = admission.refusal {
        Decision {
            save: false,
            report: reason,
        }
    } else {
        let recorded = super::activity::invocations()?;
        let counts = super::activity::activity(recorded.as_deref())?
            .unwrap_or_else(|| crate::logscan::scan(&String::from_utf8_lossy(&log)));
        let used = recorded.as_deref().and_then(UsedUnits::from_invocations);
        decide_counts(
            &counts,
            &target,
            archive.as_deref(),
            super::now_seconds(),
            budget_limit()?,
            used.as_ref(),
        )?
    };
    super::append_outputs(&format!("save={}\nsources={sources}\n", decision.save))?;
    if let Some(mismatch) = mismatch {
        return Err(CiError::new(format!(
            "{probe_log}{}{mismatch}",
            decision.report
        )));
    }
    let mut report = probe_log + &decision.report;
    if decision.save
        && let Err(error) = super::delta_snapshot::record_inventory(&target)
    {
        super::delta_snapshot::forget_inventory(&target)?;
        report =
            format!("{report}::notice::{error}; pull requests will read the whole restored base\n");
    }
    Ok(report)
}
