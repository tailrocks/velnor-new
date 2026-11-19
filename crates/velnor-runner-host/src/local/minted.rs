// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use tokio::io::AsyncWriteExt;
use tokio::sync::Mutex;

use crate::error::HostError;

use super::plan::Side;

pub(super) const MINTED_FILE: &str = "minted-agents.log";

const INTENT: &str = "+";
const RETIRED: &str = "-";
const COMPACT_SLACK: usize = 32;

type Entry = (String, String);

#[derive(Debug, Default)]
struct Journal {
    open: BTreeSet<Entry>,
    lines: usize,
}

#[derive(Debug)]
pub(super) struct MintedAgents {
    path: PathBuf,
    journal: Mutex<Journal>,
}

fn valid_name(name: &str) -> bool {
    (1..=128).contains(&name.len())
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
}

fn checksum(body: &str) -> u32 {
    body.bytes().fold(0x811c_9dc5_u32, |hash, byte| {
        (hash ^ u32::from(byte)).wrapping_mul(0x0100_0193)
    })
}

fn encode(kind: &str, side: &str, name: &str) -> String {
    let body = format!("{kind} {side} {name}");
    format!("{body} {:08x}\n", checksum(&body))
}

fn record(text: &str) -> Option<(&str, Entry)> {
    let fields: Vec<&str> = text.split(' ').collect();
    let [kind, side, name] = match fields.as_slice() {
        [kind, side, name] => [kind, side, name],
        [kind, side, name, sum] => {
            let body = &text[..text.len() - sum.len() - 1];
            if sum.len() != 8 || u32::from_str_radix(sum, 16).ok() != Some(checksum(body)) {
                return None;
            }
            [kind, side, name]
        }
        _ => return None,
    };
    if !matches!(*kind, INTENT | RETIRED) || Side::parse(side).is_none() || !valid_name(name) {
        return None;
    }
    Some((kind, ((*side).to_owned(), (*name).to_owned())))
}

fn parse(text: &str) -> (BTreeSet<Entry>, usize) {
    let mut open = BTreeSet::new();
    let mut lines = 0;
    for line in text.split_inclusive('\n') {
        let Some(line) = line.strip_suffix('\n') else {
            continue;
        };
        if line.is_empty() {
            continue;
        }
        lines += 1;
        let Some((kind, key)) = record(line) else {
            continue;
        };
        if kind == INTENT {
            open.insert(key);
        } else {
            open.remove(&key);
        }
    }
    (open, lines)
}

impl MintedAgents {
    pub(super) fn new(state: &Path) -> Self {
        Self {
            path: state.join(MINTED_FILE),
            journal: Mutex::new(Journal::default()),
        }
    }

    pub(super) async fn intent(&self, side: &str, name: &str) -> Result<(), HostError> {
        let mut journal = self.journal.lock().await;
        self.append(INTENT, side, name).await?;
        self.sync_directory().await?;
        journal.open.insert((side.to_owned(), name.to_owned()));
        journal.lines += 1;
        self.compact(&mut journal).await
    }

    pub(super) async fn retired(&self, side: &str, name: &str) -> Result<(), HostError> {
        let mut journal = self.journal.lock().await;
        self.append(RETIRED, side, name).await?;
        journal.open.remove(&(side.to_owned(), name.to_owned()));
        journal.lines += 1;
        self.compact(&mut journal).await
    }

    pub(super) async fn load(&self) -> Result<Vec<Entry>, HostError> {
        let mut journal = self.journal.lock().await;
        let text = match tokio::fs::read_to_string(&self.path).await {
            Ok(text) => text,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => String::new(),
            Err(_) => return Err(HostError::Path),
        };
        let (open, lines) = parse(&text);
        journal.open = open;
        journal.lines = lines;
        self.rewrite(&journal).await?;
        Ok(journal.open.iter().cloned().collect())
    }

    pub(super) async fn clear(&self) -> Result<(), HostError> {
        let mut journal = self.journal.lock().await;
        *journal = Journal::default();
        match tokio::fs::remove_file(&self.path).await {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(_) => Err(HostError::Path),
        }
    }

    async fn compact(&self, journal: &mut Journal) -> Result<(), HostError> {
        if journal.lines <= journal.open.len() * 2 + COMPACT_SLACK {
            return Ok(());
        }
        self.rewrite(journal).await?;
        journal.lines = journal.open.len();
        Ok(())
    }

    async fn rewrite(&self, journal: &Journal) -> Result<(), HostError> {
        if journal.open.is_empty() {
            return match tokio::fs::remove_file(&self.path).await {
                Ok(()) => Ok(()),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
                Err(_) => Err(HostError::Path),
            };
        }
        let mut text = String::new();
        for (side, name) in &journal.open {
            text.push_str(&encode(INTENT, side, name));
        }
        let fresh = self.path.with_extension("new");
        let mut file = private_options()
            .truncate(true)
            .open(&fresh)
            .await
            .map_err(|_| HostError::Path)?;
        file.write_all(text.as_bytes())
            .await
            .map_err(|_| HostError::Path)?;
        file.sync_data().await.map_err(|_| HostError::Path)?;
        drop(file);
        tokio::fs::rename(&fresh, &self.path)
            .await
            .map_err(|_| HostError::Path)?;
        self.sync_directory().await
    }

    async fn sync_directory(&self) -> Result<(), HostError> {
        let directory = self.path.parent().ok_or(HostError::Path)?;
        tokio::fs::File::open(directory)
            .await
            .map_err(|_| HostError::Path)?
            .sync_all()
            .await
            .map_err(|_| HostError::Path)
    }

    async fn append(&self, kind: &str, side: &str, name: &str) -> Result<(), HostError> {
        let mut file = private_options()
            .append(true)
            .open(&self.path)
            .await
            .map_err(|_| HostError::Path)?;
        let before = file.metadata().await.map_err(|_| HostError::Path)?.len();
        let written = async {
            file.write_all(format!("\n{}", encode(kind, side, name)).as_bytes())
                .await?;
            file.sync_data().await
        }
        .await;
        if written.is_err() {
            file.set_len(before).await.map_err(|_| HostError::Path)?;
            return Err(HostError::Path);
        }
        Ok(())
    }
}

fn private_options() -> tokio::fs::OpenOptions {
    let mut options = tokio::fs::OpenOptions::new();
    options.create(true).write(true).mode(0o600);
    options
}

#[cfg(test)]
#[path = "minted_tests.rs"]
mod tests;
