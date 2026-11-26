// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0
// SPDX-FileContributor: tailrocks/velnor-legacy@1b9fc0388925d21ff44ba1be5027684c38c8e4c9 crates/velnor-runner/src/runner.rs

use std::collections::VecDeque;
use std::path::PathBuf;

use tokio::io::{AsyncBufReadExt, AsyncSeekExt, BufReader};

use super::logs::STAMP_LEN;

const CHUNK_BYTES: usize = 256 * 1024;
const FINAL_FEED_LINES: usize = 200;

#[derive(Debug)]
pub(super) struct StepState {
    pub(super) name: String,
    pub(super) path: PathBuf,
    pub(super) taken: i64,
    offset: u64,
}

impl StepState {
    pub(super) fn new(name: String, path: PathBuf) -> Self {
        Self {
            name,
            path,
            taken: 0,
            offset: 0,
        }
    }

    pub(super) async fn next_feed_lines(&mut self) -> (i64, Vec<String>) {
        let lines = self.next_lines().await;
        self.claim(0, lines)
    }

    pub(super) async fn final_feed_lines(&mut self) -> (i64, Vec<String>) {
        let mut tail = VecDeque::new();
        let mut skipped = 0;
        loop {
            let lines = self.next_lines().await;
            if lines.is_empty() {
                break;
            }
            tail.extend(lines);
            let excess = tail.len().saturating_sub(FINAL_FEED_LINES);
            tail.drain(..excess);
            skipped += excess;
        }
        self.claim(skipped, tail.into())
    }

    fn claim(&mut self, skipped: usize, lines: Vec<String>) -> (i64, Vec<String>) {
        let start = self.taken + i64::try_from(skipped).unwrap_or(i64::MAX) + 1;
        self.taken = start - 1 + i64::try_from(lines.len()).unwrap_or(i64::MAX);
        (start, lines)
    }

    async fn next_lines(&mut self) -> Vec<String> {
        let Ok(mut file) = tokio::fs::File::open(&self.path).await else {
            return Vec::new();
        };
        if file
            .seek(std::io::SeekFrom::Start(self.offset))
            .await
            .is_err()
        {
            return Vec::new();
        }
        let mut reader = BufReader::new(file);
        let mut lines = Vec::new();
        let mut consumed = 0;
        while consumed < CHUNK_BYTES {
            let mut line = Vec::new();
            match reader.read_until(b'\n', &mut line).await {
                Ok(read) if read > 0 && line.last() == Some(&b'\n') => {
                    consumed += read;
                    self.offset += u64::try_from(read).unwrap_or(0);
                    line.pop();
                    let text = String::from_utf8_lossy(&line);
                    lines.push(text.get(STAMP_LEN..).unwrap_or_default().to_owned());
                }
                _ => break,
            }
        }
        lines
    }
}
