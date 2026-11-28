// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs::Permissions;
use std::io::{Error, ErrorKind};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use super::logs::LogFiles;
use super::{LifecycleError, RunJitOptions};
use crate::exec::private::{PRIVATE_DIR_MODE, create_dir_with_mode};
use crate::exec::{RunOptions, SHARED_DIR_MODE};

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
    create_dir_with_mode(directory, mode).await?;
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
            create_dir_with_mode(&self.root, SHARED_DIR_MODE)
                .await
                .map_err(|source| work_dir_error(&self.root, source))?;
            self.created_root = true;
            tokio::fs::set_permissions(&self.root, Permissions::from_mode(SHARED_DIR_MODE))
                .await
                .map_err(|source| work_dir_error(&self.root, source))?;
        }
        fresh_dir(&self.run.workspace, SHARED_DIR_MODE)
            .await
            .map_err(|source| work_dir_error(&self.run.workspace, source))?;
        for directory in [&self.run.temp, &self.logs] {
            fresh_dir(directory, PRIVATE_DIR_MODE)
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
