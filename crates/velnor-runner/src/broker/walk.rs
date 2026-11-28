// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::HashSet;
use std::num::NonZeroU64;
use std::path::Path;

use anyhow::Result;

use super::git::{Batch, Commit};

pub(super) struct Walk {
    pub(super) commits: Vec<String>,
    pub(super) damaged: HashSet<String>,
    pub(super) stderr: String,
}

impl Walk {
    pub(super) fn commit(sha: &str) -> Self {
        Self {
            commits: vec![sha.to_owned()],
            damaged: HashSet::new(),
            stderr: String::new(),
        }
    }
}

pub(super) async fn walk(mirror: &Path, sha: &str, count: NonZeroU64) -> Result<Walk> {
    let mut walk = Walk::commit(sha);
    if count.get() == 1 {
        return Ok(walk);
    }
    let mut batch = Batch::open(mirror)?;
    let mut seen = HashSet::from([sha.to_owned()]);
    let mut level = vec![sha.to_owned()];
    for _ in 1..count.get() {
        let read = batch.commits(&level).await?;
        let mut next = Vec::new();
        for (id, commit) in level.into_iter().zip(read) {
            match commit {
                Commit::Parents(parents) => {
                    for parent in parents {
                        if seen.insert(parent.clone()) {
                            walk.commits.push(parent.clone());
                            next.push(parent);
                        }
                    }
                }
                Commit::Damaged => {
                    walk.damaged.insert(id);
                }
            }
        }
        if next.is_empty() {
            break;
        }
        level = next;
    }
    walk.stderr = batch.finish().await;
    Ok(walk)
}
