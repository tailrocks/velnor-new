// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::io;
use std::path::Path;

use serde_json::json;
use tokio::fs::{File, OpenOptions};
use tokio::io::AsyncWriteExt;

pub(super) struct Audit {
    file: File,
    label: String,
}

impl Audit {
    pub(super) async fn open(state: &Path, label: String) -> io::Result<Self> {
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(state.join("cleanup.jsonl"))
            .await?;
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
