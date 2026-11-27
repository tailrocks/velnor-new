// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs::Permissions;
use std::io::{Error, ErrorKind};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use super::logs::LogFiles;
use super::{LifecycleError, RunJitOptions};
use crate::exec::{RunOptions, SHARED_DIR_MODE};

const PRIVATE_DIR_MODE: u32 = 0o700;

#[derive(Debug)]
pub(super) struct WorkDirs {
    root: PathBuf,
    logs: PathBuf,
    created_root: bool,
    pub(super) run: RunOptions,
}

fn work_dir_error(path: &Path, source: Error) -> LifecycleError {
    LifecycleError::WorkDir {
        path: path.to_path_buf(),
        source,
    }
}

fn absolute(path: &Path) -> Result<PathBuf, LifecycleError> {
    std::path::absolute(path).map_err(|source| work_dir_error(path, source))
}

async fn remove_dir(directory: &Path) -> std::io::Result<()> {
    match tokio::fs::remove_dir_all(directory).await {
        Err(error) if error.kind() != ErrorKind::NotFound => Err(error),
        _ => Ok(()),
    }
}

async fn fresh_dir(directory: &Path, mode: u32) -> std::io::Result<()> {
    remove_dir(directory).await?;
    tokio::fs::create_dir_all(directory).await?;
    tokio::fs::set_permissions(directory, Permissions::from_mode(mode)).await
}

impl WorkDirs {
    pub(super) fn resolve(
        options: &RunJitOptions,
        agent_name: &str,
    ) -> Result<Self, LifecycleError> {
        let root = absolute(&options.work_dir)?;
        let logs = LogFiles::dir_of(&root);
        let tool_cache = match &options.tool_cache {
            Some(path) => absolute(path)?,
            None => root.join("_tool"),
        };
        let run = RunOptions {
            workspace: root.join("workspace"),
            temp: root.join("temp"),
            tool_cache,
            runner_name: agent_name.to_owned(),
            os: options.os,
        };
        if let Some(cleared) = [&run.workspace, &run.temp, &logs]
            .into_iter()
            .find(|directory| run.tool_cache.starts_with(directory))
        {
            return Err(work_dir_error(
                &run.tool_cache,
                Error::new(
                    ErrorKind::InvalidInput,
                    format!(
                        "the tool cache is inside {}, which is cleared for every job",
                        cleared.display()
                    ),
                ),
            ));
        }
        Ok(Self {
            root,
            logs,
            created_root: false,
            run,
        })
    }

    pub(super) async fn prepare(&mut self) -> Result<LogFiles, LifecycleError> {
        let exists = tokio::fs::try_exists(&self.root)
            .await
            .map_err(|source| work_dir_error(&self.root, source))?;
        if !exists {
            tokio::fs::create_dir_all(&self.root)
                .await
                .map_err(|source| work_dir_error(&self.root, source))?;
            self.created_root = true;
            tokio::fs::set_permissions(&self.root, Permissions::from_mode(SHARED_DIR_MODE))
                .await
                .map_err(|source| work_dir_error(&self.root, source))?;
        }
        for (directory, mode) in [
            (&self.run.workspace, SHARED_DIR_MODE),
            (&self.run.temp, PRIVATE_DIR_MODE),
            (&self.logs, PRIVATE_DIR_MODE),
        ] {
            fresh_dir(directory, mode)
                .await
                .map_err(|source| work_dir_error(directory, source))?;
        }
        Ok(LogFiles::at(self.logs.clone()))
    }

    pub(super) async fn remove(&self) {
        for directory in [&self.run.workspace, &self.run.temp, &self.logs] {
            if let Err(error) = remove_dir(directory).await {
                eprintln!("warning: remove {}: {error}", directory.display());
            }
        }
        if self.created_root {
            let _ = tokio::fs::remove_dir(&self.root).await;
        }
    }

    pub(super) async fn remove_temp(&self) {
        if let Err(error) = remove_dir(&self.run.temp).await {
            eprintln!("warning: remove {}: {error}", self.run.temp.display());
        }
    }
}
