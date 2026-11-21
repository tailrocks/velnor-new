// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::process::Command;

use sha2::{Digest, Sha256};

use crate::image_candidate::hashing::hex;

const SALT: &str = "1.0";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Compression {
    Gzip,
    ZstdWithoutLong,
}

impl Compression {
    const fn name(self) -> &'static str {
        match self {
            Self::Gzip => "gzip",
            Self::ZstdWithoutLong => "zstd-without-long",
        }
    }

    #[must_use]
    pub fn detect() -> Self {
        let Ok(output) = Command::new("zstd").args(["--quiet", "--version"]).output() else {
            return Self::Gzip;
        };
        let mut text = output.stdout;
        text.extend(output.stderr);
        if text.trim_ascii().is_empty() {
            Self::Gzip
        } else {
            Self::ZstdWithoutLong
        }
    }
}

#[must_use]
pub fn cache_version(paths: &[&str], compression: Compression) -> String {
    let mut components: Vec<&str> = paths.to_vec();
    components.push(compression.name());
    components.push(SALT);
    hex(&Sha256::digest(components.join("|").as_bytes()))
}
