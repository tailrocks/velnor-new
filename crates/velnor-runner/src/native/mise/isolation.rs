// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::path::{Path, PathBuf};

use uuid::Uuid;

use crate::exec::ExecError;
use crate::native::mise_lock::AcceptedConfig;

const CONFIG: &str = "mise.toml";
const LOCK: &str = "mise.lock";
const FIXED_PATH: &str = "/usr/bin:/bin:/usr/sbin:/sbin";

pub(super) struct StoreEnv {
    pub(super) bin: PathBuf,
    pub(super) data: PathBuf,
    pub(super) shims: PathBuf,
    pub(super) cargo_home: PathBuf,
    pub(super) rustup_home: PathBuf,
}

impl StoreEnv {
    pub(super) fn directories(&self) -> [(&'static str, &Path); 4] {
        [
            ("MISE_DATA_DIR", &self.data),
            ("MISE_RUSTUP_HOME", &self.rustup_home),
            ("MISE_CARGO_HOME", &self.cargo_home),
            ("the mise binary directory", &self.bin),
        ]
    }
}

pub(super) struct Isolation {
    root: PathBuf,
    workspace: PathBuf,
}

fn text(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

fn is_boundary(next: char) -> bool {
    next.is_whitespace() || matches!(next, '/' | ':' | ',' | ';' | '"' | '\'')
}

fn io(action: &str, path: &Path) -> impl FnOnce(std::io::Error) -> ExecError {
    let context = format!("{action} {}", path.display());
    move |source| ExecError::io(context, source)
}

impl Isolation {
    pub(super) async fn create(
        temp: &Path,
        workspace: &Path,
        accepted: &AcceptedConfig,
    ) -> Result<Self, ExecError> {
        let candidate = temp.join(format!("velnor-mise-{}", Uuid::new_v4()));
        tokio::fs::DirBuilder::new()
            .mode(0o700)
            .create(&candidate)
            .await
            .map_err(io("create", &candidate))?;
        let mut isolation = Self {
            root: candidate,
            workspace: workspace.to_path_buf(),
        };
        isolation.root = tokio::fs::canonicalize(&isolation.root)
            .await
            .map_err(io("resolve", &isolation.root))?;
        for name in ["config", "home", "system", "cache"] {
            let directory = isolation.root.join(name);
            tokio::fs::create_dir(&directory)
                .await
                .map_err(io("create", &directory))?;
        }
        for (name, bytes) in [(CONFIG, &accepted.config), (LOCK, &accepted.lock)] {
            let file = isolation.config_dir().join(name);
            tokio::fs::write(&file, bytes)
                .await
                .map_err(io("write", &file))?;
        }
        Ok(isolation)
    }

    pub(super) fn config_dir(&self) -> PathBuf {
        self.root.join("config")
    }

    pub(super) fn config_file(&self) -> PathBuf {
        self.config_dir().join(CONFIG)
    }

    pub(super) fn env(&self, store: &StoreEnv) -> Vec<(String, String)> {
        let root = text(&self.root);
        let system = format!("{root}/system");
        [
            ("PATH", format!("{}:{FIXED_PATH}", text(&store.bin))),
            ("HOME", format!("{root}/home")),
            ("MISE_CEILING_PATHS", root.clone()),
            ("MISE_SYSTEM_CONFIG_DIR", system.clone()),
            ("MISE_SYSTEM_DATA_DIR", system.clone()),
            ("MISE_SYSTEM_PLUGINS_DIRS", format!("{system}/plugins")),
            ("MISE_TRUSTED_CONFIG_PATHS", text(&self.config_dir())),
            ("MISE_DATA_DIR", text(&store.data)),
            ("MISE_SHIMS_DIR", text(&store.shims)),
            ("MISE_CACHE_DIR", format!("{root}/cache")),
            ("MISE_AUTO_INSTALL", "false".to_owned()),
            ("MISE_YES", "1".to_owned()),
            ("MISE_CARGO_HOME", text(&store.cargo_home)),
            ("MISE_RUSTUP_HOME", text(&store.rustup_home)),
        ]
        .into_iter()
        .map(|(name, value)| (name.to_owned(), value))
        .collect()
    }

    pub(super) fn to_workspace(&self, value: &str) -> String {
        let private = text(&self.config_dir());
        let workspace = text(&self.workspace);
        let mut rewritten = String::with_capacity(value.len());
        let mut rest = value;
        while let Some(index) = rest.find(&private) {
            let (head, tail) = rest.split_at(index);
            let after = &tail[private.len()..];
            rewritten.push_str(head);
            let boundary = after.chars().next().is_none_or(is_boundary);
            rewritten.push_str(if boundary { &workspace } else { &private });
            rest = after;
        }
        rewritten.push_str(rest);
        rewritten
    }

    pub(super) async fn remove(self) -> Result<(), ExecError> {
        let removed = tokio::fs::remove_dir_all(&self.root).await;
        match removed {
            Err(source) if source.kind() != std::io::ErrorKind::NotFound => Err(ExecError::io(
                format!("remove {}", self.root.display()),
                source,
            )),
            _ => Ok(()),
        }
    }
}

impl Drop for Isolation {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}
