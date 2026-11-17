// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;
use std::fs;
use std::io;
use std::path::Path;
use std::time::SystemTime;

use velnor_rust::{Located, locate};

use super::Audit;

const INPUT_PATH_LIMIT: usize = 100_000;

impl Audit<'_> {
    pub(super) fn changed_since(&self, base: &Path, paths: &[String], built: SystemTime) -> bool {
        paths.iter().any(|raw| {
            let path = base.join(raw);
            match fs::metadata(&path) {
                Ok(_) => changed_tree(&path, built).unwrap_or(true),
                Err(error) => {
                    error.kind() != io::ErrorKind::NotFound
                        || match locate(self.repository, base, raw) {
                            Located::Inside(path) => self
                                .tracked
                                .range(path.clone()..)
                                .take_while(|tracked| tracked.as_str().starts_with(path.as_str()))
                                .any(|tracked| {
                                    Path::new(tracked.as_str()).starts_with(path.as_str())
                                }),
                            Located::Repository => true,
                            Located::Outside => false,
                        }
                }
            }
        })
    }
}

fn changed_tree(path: &Path, built: SystemTime) -> io::Result<bool> {
    let mut pending = vec![path.to_path_buf()];
    let mut directories = BTreeSet::new();
    let mut visited = 0;
    while let Some(path) = pending.pop() {
        visited += 1;
        if visited > INPUT_PATH_LIMIT {
            return Ok(true);
        }
        let metadata = fs::metadata(&path)?;
        if metadata.modified()? > built {
            return Ok(true);
        }
        if metadata.is_dir() && directories.insert(fs::canonicalize(&path)?) {
            for entry in fs::read_dir(path)? {
                if visited + pending.len() >= INPUT_PATH_LIMIT {
                    return Ok(true);
                }
                pending.push(entry?.path());
            }
        }
    }
    Ok(false)
}
