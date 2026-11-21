// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

//! The `version` the cache service stores with an entry. Two entries may share one key on one ref
//! when their versions differ, so naming a key does not name one entry.
//!
//! `actions/cache` computes it as the SHA-256 of the cached paths, the compression method and a
//! salt, joined with `|` (`getCacheVersion` in `@actions/toolkit`). The compression method is a
//! property of the runner that saves or restores: zstd when the runner has `zstd`, else gzip.

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

    /// What `actions/cache` picks on this runner: it runs `zstd --quiet --version` and takes any
    /// output, standard or error, as a sign that zstd works.
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
