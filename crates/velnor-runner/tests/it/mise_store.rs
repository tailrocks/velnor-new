// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result};

use crate::mise_world::World;

#[derive(Clone, Copy, Debug)]
pub(crate) enum Dir {
    Data,
    Rustup,
    Cargo,
    Binary,
}

impl Dir {
    pub(crate) fn role(self) -> &'static str {
        match self {
            Self::Data => "MISE_DATA_DIR",
            Self::Rustup => "MISE_RUSTUP_HOME",
            Self::Cargo => "MISE_CARGO_HOME",
            Self::Binary => "the mise binary directory",
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) enum StoreMode {
    Sealed,
    Writable(Dir),
    Missing(Dir),
    Real,
    Bits,
}

fn chmod(path: &Path, mode: u32) -> Result<()> {
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode))
        .with_context(|| format!("chmod {mode:o} {}", path.display()))
}

pub(crate) fn link_runtime_versions(installs: &Path, version: &str) -> Result<()> {
    let mut names = vec!["latest".to_owned()];
    let parts: Vec<&str> = version.split('.').collect();
    names.extend((1..parts.len()).map(|count| parts[..count].join(".")));
    for name in names {
        let link = installs.join(name);
        if link.symlink_metadata().is_err() {
            std::os::unix::fs::symlink(format!("./{version}"), &link)
                .with_context(|| format!("link {}", link.display()))?;
        }
    }
    Ok(())
}

fn walk(root: &Path, mode: u32) -> Result<()> {
    let meta =
        std::fs::symlink_metadata(root).with_context(|| format!("stat {}", root.display()))?;
    if !meta.is_dir() {
        return Ok(());
    }
    chmod(root, mode)?;
    for entry in std::fs::read_dir(root).with_context(|| format!("list {}", root.display()))? {
        walk(&entry?.path(), mode)?;
    }
    Ok(())
}

impl World {
    pub(crate) fn data_dir(&self) -> PathBuf {
        self.store.join("tools/data")
    }

    pub(crate) fn rustup_dir(&self) -> PathBuf {
        self.store.join("tools/rustup")
    }

    pub(crate) fn cargo_dir(&self) -> PathBuf {
        self.store.join("tools/cargo")
    }

    pub(crate) fn dir(&self, dir: Dir) -> PathBuf {
        match dir {
            Dir::Data => self.data_dir(),
            Dir::Rustup => self.rustup_dir(),
            Dir::Cargo => self.cargo_dir(),
            Dir::Binary => self.binary_dir(),
        }
    }

    fn homes(&self) -> [PathBuf; 3] {
        [self.data_dir(), self.rustup_dir(), self.cargo_dir()]
    }

    fn present(&self, directory: &Path) -> bool {
        !matches!(self.store_mode, StoreMode::Missing(dir) if self.dir(dir) == directory)
    }

    pub(crate) fn seal_store(&self) -> Result<()> {
        for directory in self.homes().iter().filter(|home| self.present(home)) {
            std::fs::create_dir_all(directory)
                .with_context(|| format!("create {}", directory.display()))?;
        }
        if self.data_dir().is_dir() {
            for name in ["installs", "downloads"] {
                std::fs::create_dir_all(self.data_dir().join(name))
                    .context("create the data directory")?;
            }
        }
        for home in self.homes() {
            if !home.is_dir() {
                continue;
            }
            match self.store_mode {
                StoreMode::Real => chmod(&home, 0o555)?,
                _ => walk(&home, 0o555)?,
            }
        }
        for directory in [self.store.join("tools"), self.store.clone()] {
            chmod(&directory, 0o555)?;
        }
        Ok(())
    }

    pub(crate) fn unseal_store(&self) -> Result<()> {
        for directory in [self.store.clone(), self.store.join("tools")] {
            if directory.exists() {
                chmod(&directory, 0o755)?;
            }
        }
        for home in self.homes().iter().filter(|home| home.is_dir()) {
            walk(home, 0o755)?;
        }
        Ok(())
    }

    pub(crate) fn declare_read_only(&self, command: &mut Command) {
        let mut declared: Vec<PathBuf> = self.homes().to_vec();
        declared.push(self.binary_dir());
        match self.store_mode {
            StoreMode::Real | StoreMode::Bits => return,
            StoreMode::Writable(dir) | StoreMode::Missing(dir) => {
                declared.retain(|directory| *directory != self.dir(dir));
            }
            StoreMode::Sealed => {}
        }
        if let Ok(list) = std::env::join_paths(declared) {
            command.env("VELNOR_TEST_READ_ONLY_DIRS", list);
        }
    }
}

impl Drop for World {
    fn drop(&mut self) {
        let _ = self.unseal_store();
    }
}
