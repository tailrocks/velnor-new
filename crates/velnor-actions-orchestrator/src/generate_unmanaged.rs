// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::path::{Path, PathBuf};

use velnor_actions_contract::{GithubConfig, UnmanagedPolicy};

use super::preserve::{
    generated_marked_dco, generated_marked_script, generated_shared_action, generator_owned,
    retired_marked_lint_config,
};
use crate::OrchestratorError;

pub(crate) fn list(
    root: &Path,
    preserve_workflows: &[String],
    github: &GithubConfig,
) -> Result<Vec<String>, OrchestratorError> {
    let base = root.join(".github");
    match std::fs::symlink_metadata(&base) {
        Ok(meta) if meta.is_symlink() || !meta.is_dir() => {
            return Err(OrchestratorError::UnsafePath {
                path: base.display().to_string(),
                reason: "github_not_directory".to_owned(),
            });
        }
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(io(&base, &error)),
    }
    let mut found = Vec::new();
    walk(&base, Path::new(""), preserve_workflows, &mut found)?;
    let mut paths: Vec<String> = found
        .into_iter()
        .map(|relative| {
            let parts: Vec<_> = relative.iter().map(|part| part.to_string_lossy()).collect();
            format!(".github/{}", parts.join("/"))
        })
        .filter(|path| !github.is_allowed(path))
        .filter(|path| !preserve_workflows.iter().any(|preserved| preserved == path))
        .collect();
    paths.sort();
    Ok(paths)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnmanagedReport {
    pub policy: UnmanagedPolicy,
    pub entries: Vec<String>,
}

impl UnmanagedReport {
    #[must_use]
    pub fn is_denied(&self) -> bool {
        self.policy == UnmanagedPolicy::Deny && !self.entries.is_empty()
    }
}

pub fn unmanaged_report(root: &Path) -> Result<UnmanagedReport, OrchestratorError> {
    let config = crate::config::load_config(root)?;
    let entries = list(root, &config.workflow.preserve_workflows, &config.github)?;
    Ok(UnmanagedReport {
        policy: config.github.unmanaged,
        entries,
    })
}

fn walk(
    base: &Path,
    relative: &Path,
    preserve_workflows: &[String],
    found: &mut Vec<PathBuf>,
) -> Result<bool, OrchestratorError> {
    let source = base.join(relative);
    let metadata = std::fs::symlink_metadata(&source).map_err(|error| io(&source, &error))?;
    if !relative.as_os_str().is_empty()
        && (generator_owned(relative, preserve_workflows)
            || generated_shared_action(relative, &source, &metadata)?
            || generated_marked_script(relative, &source, &metadata)?
            || generated_marked_dco(relative, &source, &metadata)?
            || retired_marked_lint_config(relative, &source, &metadata)?)
    {
        return Ok(false);
    }
    if !metadata.is_dir() || metadata.is_symlink() {
        found.push(relative.to_path_buf());
        return Ok(true);
    }
    let mut had_entry = false;
    let mut kept = false;
    for entry in std::fs::read_dir(&source).map_err(|error| io(&source, &error))? {
        let entry = entry.map_err(|error| io(&source, &error))?;
        had_entry = true;
        kept |= walk(
            base,
            &relative.join(entry.file_name()),
            preserve_workflows,
            found,
        )?;
    }
    if !had_entry && !relative.as_os_str().is_empty() {
        found.push(relative.to_path_buf());
        return Ok(true);
    }
    Ok(kept)
}

fn io(path: &Path, error: &std::io::Error) -> OrchestratorError {
    OrchestratorError::io(path.display().to_string(), error.to_string())
}
