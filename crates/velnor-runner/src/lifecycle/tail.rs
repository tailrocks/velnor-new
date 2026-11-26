// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0
// SPDX-FileContributor: tailrocks/velnor-legacy@1b9fc0388925d21ff44ba1be5027684c38c8e4c9 crates/velnor-runner/src/runner.rs

use std::path::PathBuf;

use tokio::io::{AsyncBufReadExt, AsyncSeekExt, BufReader};

use super::logs::STAMP_LEN;

const CHUNK_BYTES: usize = 256 * 1024;

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

    pub(super) async fn next_lines(&mut self) -> Vec<String> {
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
