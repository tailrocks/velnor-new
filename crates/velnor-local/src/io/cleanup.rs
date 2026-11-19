// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::io::{self, BufRead, BufReader, Write};
use std::path::Path;

use serde_json::json;
use tokio::fs::File;
use tokio::io::AsyncWriteExt;

pub(super) struct Audit {
    file: File,
    label: String,
}

fn recover(path: &Path) -> io::Result<std::fs::File> {
    let file = std::fs::OpenOptions::new()
        .create(true)
        .read(true)
        .append(true)
        .open(path)?;
    file.lock()?;
    let mut reader = BufReader::new(file);
    let mut offset = 0;
    let mut line = Vec::new();
    loop {
        line.clear();
        let length = reader.read_until(b'\n', &mut line)?;
        if length == 0 {
            return Ok(reader.into_inner());
        }
        let parsed = serde_json::from_slice::<serde_json::Value>(&line);
        if line.last() != Some(&b'\n') {
            let mut file = reader.into_inner();
            if parsed.is_ok() {
                file.write_all(b"\n")?;
            } else {
                file.set_len(offset)?;
            }
            return Ok(file);
        }
        parsed.map_err(io::Error::other)?;
        offset += u64::try_from(length).map_err(io::Error::other)?;
    }
}

impl Audit {
    pub(super) async fn open(state: &Path, label: String) -> io::Result<Self> {
        let path = state.join("cleanup.jsonl");
        let file = tokio::task::spawn_blocking(move || recover(&path))
            .await
            .map_err(io::Error::other)??;
        let file = File::from_std(file);
        let mut audit = Self { file, label };
        audit.record("intent", None).await?;
        Ok(audit)
    }

    pub(super) async fn record(
        &mut self,
        event: &str,
        resource: Option<(&str, &str)>,
    ) -> io::Result<()> {
        let mut line = json!({"event": event, "filters": {"label": [&self.label]}});
        if let Some((kind, id)) = resource {
            line["resource"] = json!(kind);
            line["id"] = json!(id);
        }
        self.file.write_all(format!("{line}\n").as_bytes()).await?;
        self.file.flush().await
    }
}
