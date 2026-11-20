// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::QualificationError as HostError;
use crate::probes::DaemonIdentity;
use crate::resources::TrackedContainer;

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct State {
    pub(super) daemon_id: String,
    pub(super) daemon_root: String,
    pub(super) daemon_version: String,
    pub(super) run_id: String,
    pub(super) containers: Vec<TrackedContainer>,
    pub(super) volumes: Vec<String>,
    pub(super) settled: Vec<String>,
    pub(super) deleting: Vec<String>,
}

pub(super) struct Journal {
    path: PathBuf,
    directory: File,
    lock: File,
    lock_path: PathBuf,
}

fn private_file(file: &File) -> Result<(), HostError> {
    let metadata = file.metadata().map_err(|_| HostError::Ownership)?;
    if !metadata.is_file()
        || metadata.uid() != rustix::process::geteuid().as_raw()
        || metadata.mode() & 0o077 != 0
        || metadata.nlink() != 1
    {
        return Err(HostError::Ownership);
    }
    Ok(())
}

fn open_private(path: &Path) -> Result<File, HostError> {
    let metadata = fs::symlink_metadata(path).map_err(|_| HostError::Ownership)?;
    if !metadata.is_file() {
        return Err(HostError::Ownership);
    }
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .open(path)
        .map_err(|_| HostError::Ownership)?;
    private_file(&file)?;
    let opened = file.metadata().map_err(|_| HostError::Ownership)?;
    if metadata.dev() != opened.dev() || metadata.ino() != opened.ino() {
        return Err(HostError::Ownership);
    }
    Ok(file)
}

impl Journal {
    pub(super) fn open(
        path: &Path,
        daemon: &DaemonIdentity,
    ) -> Result<(Self, Option<State>), HostError> {
        if !path.is_absolute() {
            return Err(HostError::Ownership);
        }
        let parent = path.parent().ok_or(HostError::Ownership)?;
        if fs::canonicalize(parent).map_err(|_| HostError::Ownership)? != parent {
            return Err(HostError::Ownership);
        }
        let metadata = fs::symlink_metadata(parent).map_err(|_| HostError::Ownership)?;
        if !metadata.is_dir()
            || metadata.uid() != rustix::process::geteuid().as_raw()
            || metadata.permissions().mode() & 0o077 != 0
        {
            return Err(HostError::Ownership);
        }
        let directory = File::open(parent).map_err(|_| HostError::Ownership)?;
        let opened = directory.metadata().map_err(|_| HostError::Ownership)?;
        if metadata.dev() != opened.dev() || metadata.ino() != opened.ino() {
            return Err(HostError::Ownership);
        }
        let lock_path = path.with_extension("lock");
        let lock = match OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&lock_path)
        {
            Ok(file) => {
                directory.sync_all().map_err(|_| HostError::Ownership)?;
                file
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                open_private(&lock_path)?
            }
            Err(_) => return Err(HostError::Ownership),
        };
        private_file(&lock)?;
        lock.try_lock().map_err(|_| HostError::Ownership)?;
        let state = match fs::symlink_metadata(path) {
            Ok(_) => {
                let mut file = open_private(path)?;
                if file.metadata().map_err(|_| HostError::Ownership)?.len() > 1_048_576 {
                    return Err(HostError::Ownership);
                }
                let mut bytes = Vec::new();
                file.read_to_end(&mut bytes)
                    .map_err(|_| HostError::Ownership)?;
                let state: State =
                    serde_json::from_slice(&bytes).map_err(|_| HostError::Ownership)?;
                if state.daemon_id != daemon.id
                    || state.daemon_root != daemon.root
                    || state.daemon_version != daemon.version
                {
                    return Err(HostError::Ownership);
                }
                Some(state)
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
            Err(_) => return Err(HostError::Ownership),
        };
        Ok((
            Self {
                path: path.to_owned(),
                directory,
                lock,
                lock_path,
            },
            state,
        ))
    }

    fn verify_location(&self) -> Result<(), HostError> {
        let parent = self.path.parent().ok_or(HostError::Ownership)?;
        if fs::canonicalize(parent).map_err(|_| HostError::Ownership)? != parent {
            return Err(HostError::Ownership);
        }
        let metadata = fs::symlink_metadata(parent).map_err(|_| HostError::Ownership)?;
        let opened = self
            .directory
            .metadata()
            .map_err(|_| HostError::Ownership)?;
        if !metadata.is_dir()
            || metadata.uid() != rustix::process::geteuid().as_raw()
            || metadata.mode() & 0o077 != 0
            || metadata.dev() != opened.dev()
            || metadata.ino() != opened.ino()
        {
            return Err(HostError::Ownership);
        }
        let current = open_private(&self.lock_path)?;
        let held = self.lock.metadata().map_err(|_| HostError::Ownership)?;
        let current = current.metadata().map_err(|_| HostError::Ownership)?;
        if held.dev() != current.dev() || held.ino() != current.ino() {
            return Err(HostError::Ownership);
        }
        match fs::symlink_metadata(&self.path) {
            Ok(_) => {
                open_private(&self.path)?;
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => return Err(HostError::Ownership),
        }
        Ok(())
    }

    pub(super) fn save(&self, state: &State) -> Result<(), HostError> {
        self.verify_location()?;
        let mut random = [0_u8; 16];
        getrandom::fill(&mut random).map_err(|_| HostError::Ownership)?;
        let mut suffix = String::with_capacity(32);
        for byte in random {
            use std::fmt::Write as _;
            write!(&mut suffix, "{byte:02x}").map_err(|_| HostError::Ownership)?;
        }
        let temporary = self.path.with_extension(format!("tmp-{suffix}"));
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&temporary)
            .map_err(|_| HostError::Ownership)?;
        let result = (|| {
            serde_json::to_writer(&mut file, state).map_err(|_| HostError::Ownership)?;
            file.flush().map_err(|_| HostError::Ownership)?;
            file.sync_all().map_err(|_| HostError::Ownership)?;
            fs::rename(&temporary, &self.path).map_err(|_| HostError::Ownership)?;
            self.directory.sync_all().map_err(|_| HostError::Ownership)
        })();
        if result.is_err() && fs::remove_file(temporary).is_err() {
            return Err(HostError::Ownership);
        }
        result
    }

    pub(super) fn clear(&self) -> Result<(), HostError> {
        self.verify_location()?;
        fs::remove_file(&self.path).map_err(|_| HostError::Ownership)?;
        self.directory.sync_all().map_err(|_| HostError::Ownership)
    }
}
