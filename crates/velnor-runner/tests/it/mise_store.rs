// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;

use anyhow::{Context, Result};

use crate::mise_world::World;

#[derive(Clone, Copy)]
pub(crate) enum StoreMode {
    Sealed,
    Open(&'static str),
    NoData,
}

impl World {
    pub(crate) fn data_dir(&self) -> PathBuf {
        self.store.join("tools/data")
    }

    fn sealed_dirs(&self) -> [PathBuf; 3] {
        [
            self.data_dir(),
            self.store.join("tools"),
            self.store.clone(),
        ]
    }

    pub(crate) fn seal_store(&self) -> Result<()> {
        let data = self.data_dir();
        if !matches!(self.store_mode, StoreMode::NoData) {
            for name in ["installs", "downloads"] {
                std::fs::create_dir_all(data.join(name)).context("create the data directory")?;
            }
        }
        let open = match self.store_mode {
            StoreMode::Open(relative) => Some(self.store.join(relative)),
            _ => None,
        };
        for directory in self.sealed_dirs() {
            if directory.exists() && open.as_ref() != Some(&directory) {
                std::fs::set_permissions(&directory, std::fs::Permissions::from_mode(0o555))
                    .with_context(|| format!("seal {}", directory.display()))?;
            }
        }
        Ok(())
    }

    pub(crate) fn unseal_store(&self) -> Result<()> {
        for directory in self.sealed_dirs().iter().rev() {
            if directory.exists() {
                std::fs::set_permissions(directory, std::fs::Permissions::from_mode(0o755))
                    .with_context(|| format!("unseal {}", directory.display()))?;
            }
        }
        Ok(())
    }
}

impl Drop for World {
    fn drop(&mut self) {
        let _ = self.unseal_store();
    }
}
