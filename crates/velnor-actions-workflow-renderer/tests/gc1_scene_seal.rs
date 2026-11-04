// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;
use std::process::Command;

use crate::gc1_scene::{Scene, fail};

const BIND_LIMIT_BYTES: u64 = 1_611_661_312;

impl Scene {
    pub(crate) fn payload_digest(&self) -> Result<String, Box<dyn Error>> {
        let output = Command::new("sha256sum").arg(self.payload()).output()?;
        String::from_utf8(output.stdout)?
            .split_whitespace()
            .next()
            .map(|digest| format!("s256-{digest}"))
            .ok_or_else(|| fail("sha256sum printed nothing"))
    }

    pub(crate) fn bind_to_payload(&self, matched: &str) -> String {
        let path = self.payload();
        let small = std::fs::symlink_metadata(&path)
            .is_ok_and(|meta| meta.is_file() && meta.len() <= BIND_LIMIT_BYTES);
        if matched.is_empty() || !small {
            return matched.to_owned();
        }
        let base = match matched.rsplit_once("-s256-") {
            Some((base, tail)) if tail.len() == 64 => base,
            _ => matched,
        };
        self.payload_digest()
            .map_or_else(|_| matched.to_owned(), |digest| format!("{base}-{digest}"))
    }
}
