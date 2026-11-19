// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use crate::cli::{RunArgs, SourcesFetchArgs};
use crate::error::CiError;
use crate::escape::stderr_line;

pub(super) fn fetch(args: &SourcesFetchArgs) -> ExitCode {
    match fetch_roots(&args.roots) {
        Ok(()) => ExitCode::SUCCESS,
        Err((code, message)) => {
            eprintln!("velnor-ci: {}", stderr_line(&message));
            ExitCode::from(code)
        }
    }
}

fn fetch_roots(roots: &[PathBuf]) -> Result<(), (u8, String)> {
    let base = super::current_dir().map_err(|e| (1, e.to_string()))?;
    let directories = resolve(&base, roots).map_err(|e| (1, e.to_string()))?;
    let command = ["cargo", "fetch", "--locked"].map(str::to_owned).to_vec();
    let args = RunArgs {
        require_doctests: false,
        command,
    };
    for (root, directory) in roots.iter().zip(&directories) {
        std::env::set_current_dir(directory)
            .map_err(|e| (1, format!("cannot enter root {}: {e}", root.display())))?;
        let result = super::run::execute(&args);
        std::env::set_current_dir(&base)
            .map_err(|e| (1, format!("cannot return to {}: {e}", base.display())))?;
        match result {
            Ok(0) => {}
            Ok(code) => return Err((code, failed(root, &format!("cargo fetch exited {code}")))),
            Err((code, message)) => return Err((code, failed(root, &message))),
        }
    }
    Ok(())
}

fn failed(root: &Path, reason: &str) -> String {
    format!("sources fetch failed in {}: {reason}", root.display())
}

fn resolve(base: &Path, roots: &[PathBuf]) -> Result<Vec<PathBuf>, CiError> {
    let mut seen: BTreeMap<PathBuf, &Path> = BTreeMap::new();
    let mut directories = Vec::with_capacity(roots.len());
    for root in roots {
        let directory = base
            .join(root)
            .canonicalize()
            .ok()
            .filter(|path| path.is_dir())
            .ok_or_else(|| CiError::new(format!("root {} is not a directory", root.display())))?;
        if let Some(first) = seen.insert(directory.clone(), root) {
            return Err(CiError::new(format!(
                "duplicate root {}: {} names the same directory",
                root.display(),
                first.display()
            )));
        }
        directories.push(directory);
    }
    Ok(directories)
}
