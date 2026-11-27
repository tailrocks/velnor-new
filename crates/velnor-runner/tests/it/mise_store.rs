// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result};

use crate::mise_real::canonical;
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
pub(crate) enum Ancestor {
    Root,
    Tools,
}

#[derive(Clone, Copy, Debug)]
pub(crate) enum StoreMode {
    Sealed,
    Loose(Ancestor, u32),
    Writable(Dir),
    Missing(Dir),
    Linked(Dir),
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

    fn ancestor(&self, ancestor: Ancestor) -> PathBuf {
        match ancestor {
            Ancestor::Root => self.root.clone(),
            Ancestor::Tools => self.store.join("tools"),
        }
    }

    pub(crate) fn trust_declaration(&self) -> String {
        let owner = std::fs::metadata(&self.root).map_or(0, |meta| meta.uid());
        format!("{owner}:{}", canonical(&self.root).display())
    }

    fn homes(&self) -> [PathBuf; 3] {
        [self.data_dir(), self.rustup_dir(), self.cargo_dir()]
    }

    fn linked(&self, directory: &Path) -> bool {
        matches!(self.store_mode, StoreMode::Linked(dir) if self.dir(dir) == directory)
    }

    fn link_home(&self, directory: &Path) -> Result<()> {
        let real = self
            .root
            .join("linked")
            .join(directory.file_name().context("the home name")?);
        std::fs::create_dir_all(&real).context("create the linked home")?;
        if let Some(parent) = directory.parent() {
            std::fs::create_dir_all(parent).context("create the tools directory")?;
        }
        if directory.symlink_metadata().is_err() {
            std::os::unix::fs::symlink(&real, directory).context("link the home")?;
        }
        Ok(())
    }

    fn present(&self, directory: &Path) -> bool {
        !matches!(self.store_mode, StoreMode::Missing(dir) if self.dir(dir) == directory)
    }

    pub(crate) fn seal_store(&self) -> Result<()> {
        for directory in self.homes().iter().filter(|home| self.present(home)) {
            if self.linked(directory) {
                self.link_home(directory)?;
            } else {
                std::fs::create_dir_all(directory)
                    .with_context(|| format!("create {}", directory.display()))?;
            }
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
        let binary = self.binary_dir();
        if binary.is_dir() && !matches!(self.store_mode, StoreMode::Writable(Dir::Binary)) {
            chmod(&binary, 0o555)?;
        }
        for directory in [
            self.store.join("tools/mise"),
            self.store.join("tools"),
            self.store.clone(),
        ] {
            if directory.is_dir() {
                chmod(&directory, 0o555)?;
            }
        }
        if let StoreMode::Loose(ancestor, mode) = self.store_mode {
            chmod(&self.ancestor(ancestor), mode)?;
        }
        Ok(())
    }

    pub(crate) fn unseal_store(&self) -> Result<()> {
        let tools = self.store.join("tools");
        for directory in [
            self.root.clone(),
            self.store.clone(),
            tools.clone(),
            tools.join("mise"),
            self.binary_dir(),
        ] {
            if directory.exists() {
                chmod(&directory, 0o755)?;
            }
        }
        for home in self.homes().iter().filter(|home| home.is_dir()) {
            walk(home, 0o755)?;
        }
        Ok(())
    }

    fn real(&self, dir: Dir) -> PathBuf {
        let relative = self.dir(dir);
        let relative = relative.strip_prefix(&self.store).unwrap_or(&relative);
        canonical(&self.store).join(relative)
    }

    pub(crate) fn declare_read_only(&self, command: &mut Command) {
        command.env("VELNOR_TEST_STORE_TRUST", self.trust_declaration());
        let table = self.root.join("mountinfo-default");
        if std::fs::write(&table, "1 0 0:1 / / rw - ext4 /dev/a rw\n").is_ok() {
            command.env("VELNOR_TEST_MOUNTINFO", table);
        }
        let mut declared: Vec<PathBuf> = [Dir::Data, Dir::Rustup, Dir::Cargo, Dir::Binary]
            .map(|dir| self.real(dir))
            .to_vec();
        match self.store_mode {
            StoreMode::Real | StoreMode::Bits => return,
            StoreMode::Writable(dir) | StoreMode::Missing(dir) => {
                declared.retain(|directory| *directory != self.real(dir));
            }
            StoreMode::Sealed | StoreMode::Loose(..) | StoreMode::Linked(_) => {}
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
