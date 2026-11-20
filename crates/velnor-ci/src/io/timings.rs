// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::path::Path;

use velnor_gen::io::read_checkout;
use velnor_rust::{TIMINGS_PATH, render_timings};

use super::{api_url, current_dir, http, token};
use crate::cli::TimingsArgs;
use crate::error::CiError;
use crate::github::Repo;
use crate::timings::{TimingsReport, current_setup, gather};

pub(super) fn run_timings(args: &TimingsArgs) -> Result<String, CiError> {
    let token = token()?;
    let repo = Repo::parse(&args.repo)?;
    let root = current_dir()?;
    let checkout = read_checkout(&root).map_err(|e| CiError::new(e.to_string()))?;
    let (roots, runner) = current_setup(&checkout.view)?;
    let report = gather(
        &http::HttpGitHub::new(api_url(), token, http::DEFAULT_TIMEOUT),
        &repo,
        &roots,
        &runner,
    )?;
    write_report(&checkout.root, report)
}

pub fn write_report(root: &Path, report: TimingsReport) -> Result<String, CiError> {
    let mut lines = report.notes;
    if report.timings.is_empty() {
        lines.push(remove_stale(root)?);
    } else {
        write_file(root, &render_timings(&report.timings))?;
        lines.push(format!(
            "wrote {TIMINGS_PATH} for {} Cargo roots",
            report.timings.len()
        ));
    }
    Ok(lines.join("\n") + "\n")
}

fn remove_stale(root: &Path) -> Result<String, CiError> {
    let file = root.join(TIMINGS_PATH.as_str());
    match fs::remove_file(&file) {
        Ok(()) => Ok(format!("{TIMINGS_PATH} was removed")),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            Ok(format!("{TIMINGS_PATH} was not written"))
        }
        Err(e) => Err(CiError::new(format!(
            "cannot remove {}: {e}",
            file.display()
        ))),
    }
}

fn write_file(root: &Path, text: &str) -> Result<(), CiError> {
    let file = root.join(TIMINGS_PATH.as_str());
    let failed = |e: std::io::Error| CiError::new(format!("cannot write {}: {e}", file.display()));
    if let Some(parent) = file.parent() {
        fs::create_dir_all(parent).map_err(failed)?;
    }
    fs::write(&file, text).map_err(failed)
}
