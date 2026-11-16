// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use tokio::io::AsyncWriteExt;

use crate::error::HostError;

pub(super) const MINTED_FILE: &str = "minted-agents.log";

const MINTED: &str = "+";
const RETIRED: &str = "-";

#[derive(Debug)]
pub(super) struct MintedAgents {
    path: PathBuf,
}

impl MintedAgents {
    pub(super) fn new(state: &Path) -> Self {
        Self {
            path: state.join(MINTED_FILE),
        }
    }

    pub(super) async fn minted(&self, side: &str, name: &str) -> Result<(), HostError> {
        self.append(MINTED, side, name).await
    }

    pub(super) async fn retired(&self, side: &str, name: &str) -> Result<(), HostError> {
        self.append(RETIRED, side, name).await
    }

    pub(super) async fn unretired(&self) -> Result<Vec<(String, String)>, HostError> {
        let text = match tokio::fs::read_to_string(&self.path).await {
            Ok(text) => text,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => String::new(),
            Err(_) => return Err(HostError::Path),
        };
        let mut open = BTreeSet::new();
        for line in text.lines() {
            let mut fields = line.splitn(3, ' ');
            let (Some(kind), Some(side), Some(name)) =
                (fields.next(), fields.next(), fields.next())
            else {
                continue;
            };
            let key = (side.to_owned(), name.to_owned());
            if kind == MINTED {
                open.insert(key);
            } else if kind == RETIRED {
                open.remove(&key);
            }
        }
        Ok(open.into_iter().collect())
    }

    pub(super) async fn clear(&self) -> Result<(), HostError> {
        match tokio::fs::remove_file(&self.path).await {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(_) => Err(HostError::Path),
        }
    }

    async fn append(&self, kind: &str, side: &str, name: &str) -> Result<(), HostError> {
        let mut file = tokio::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)
            .await
            .map_err(|_| HostError::Path)?;
        file.write_all(format!("{kind} {side} {name}\n").as_bytes())
            .await
            .map_err(|_| HostError::Path)?;
        file.sync_data().await.map_err(|_| HostError::Path)
    }
}
