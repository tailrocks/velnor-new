// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::HashSet;
use std::io;
use std::path::{Path, PathBuf};

use tokio::fs;
use tokio::io::AsyncWriteExt;

use crate::pin::Sha256;

const MARKER: &str = ".git/velnor-timeout-scaled";

/// The workflow texts of a run copy that already carry the timeout factor, so that a branch the
/// driver switches to again is never scaled twice.
pub(super) struct Scaled {
    path: PathBuf,
    seen: HashSet<String>,
}

impl Scaled {
    pub(super) async fn open(repo: &Path) -> io::Result<Self> {
        let path = repo.join(MARKER);
        let seen = match fs::read_to_string(&path).await {
            Ok(text) => text.lines().map(str::to_owned).collect(),
            Err(error) if error.kind() == io::ErrorKind::NotFound => HashSet::new(),
            Err(error) => return Err(error),
        };
        Ok(Self { path, seen })
    }

    pub(super) fn holds(&self, text: &str) -> bool {
        self.seen.contains(Sha256::of(text.as_bytes()).as_str())
    }

    pub(super) async fn record(&mut self, text: &str) -> io::Result<()> {
        let digest = Sha256::of(text.as_bytes()).as_str().to_owned();
        let mut file = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)
            .await?;
        file.write_all(format!("{digest}\n").as_bytes()).await?;
        file.flush().await?;
        self.seen.insert(digest);
        Ok(())
    }
}
