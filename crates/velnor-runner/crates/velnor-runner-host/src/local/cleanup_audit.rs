// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::HashMap;
use std::path::Path;

use serde_json::json;
use tokio::fs::{File, OpenOptions};
use tokio::io::AsyncWriteExt;

use crate::error::HostError;

pub(super) struct Audit {
    file: File,
    filters: HashMap<String, Vec<String>>,
}

impl Audit {
    pub(super) async fn open(
        path: &Path,
        filters: HashMap<String, Vec<String>>,
    ) -> Result<Self, HostError> {
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .await
            .map_err(|_| HostError::Cleanup)?;
        let mut audit = Self { file, filters };
        audit.record("intent", None).await?;
        Ok(audit)
    }

    pub(super) async fn record(
        &mut self,
        event: &str,
        resource: Option<(&str, &str)>,
    ) -> Result<(), HostError> {
        let mut line = json!({"event": event, "filters": self.filters});
        if let Some((kind, id)) = resource {
            line["resource"] = json!(kind);
            line["id"] = json!(id);
        }
        self.file
            .write_all(format!("{line}\n").as_bytes())
            .await
            .map_err(|_| HostError::Cleanup)?;
        self.file.flush().await.map_err(|_| HostError::Cleanup)
    }
}
