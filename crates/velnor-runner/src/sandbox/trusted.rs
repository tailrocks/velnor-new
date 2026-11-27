// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::io::Read;
use std::path::Path;

use anyhow::{Context, Result, ensure};
use serde::Deserialize;

const LIMIT: u64 = 64 * 1024;

#[derive(Debug, Deserialize)]
pub(super) struct Header {
    repository_id: u64,
    repository: String,
    sha: String,
    event_name: String,
}

impl Header {
    pub(super) fn parse(bytes: &[u8]) -> Result<Self> {
        let header: Self = serde_json::from_slice(bytes).context("parse the trusted facts")?;
        ensure!(
            header.repository_id != 0
                && header
                    .repository
                    .split_once('/')
                    .is_some_and(|(owner, name)| !owner.is_empty() && !name.contains('/')),
            "the trusted facts name no repository"
        );
        ensure!(
            header.sha.len() == 40 && header.sha.bytes().all(|byte| byte.is_ascii_hexdigit()),
            "the trusted facts carry no commit"
        );
        ensure!(
            !header.event_name.is_empty(),
            "the trusted facts carry no event"
        );
        Ok(header)
    }

    pub(super) fn read(path: &Path) -> Result<Self> {
        let file = std::fs::File::open(path)
            .with_context(|| format!("open the trusted facts {}", path.display()))?;
        let mut bytes = Vec::new();
        file.take(LIMIT + 1)
            .read_to_end(&mut bytes)
            .with_context(|| format!("read the trusted facts {}", path.display()))?;
        ensure!(
            u64::try_from(bytes.len()).is_ok_and(|length| length <= LIMIT),
            "the trusted facts are larger than {LIMIT} bytes"
        );
        Self::parse(&bytes)
    }

    pub(super) fn describe(&self) -> String {
        format!(
            "repository {}({}) sha {} event {}",
            self.repository,
            self.repository_id,
            self.sha.get(..12).unwrap_or(&self.sha),
            self.event_name
        )
    }
}
