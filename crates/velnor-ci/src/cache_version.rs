// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::ffi::OsStr;
use std::fs;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::process::Command;

use sha2::{Digest, Sha256};

use crate::image_candidate::hashing::hex;
use crate::tools_local::HOME_PATHS;

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
    pub fn detect(home: &Path) -> Self {
        Self::detect_in(std::env::var_os("PATH").as_deref(), home)
    }

    #[must_use]
    pub fn detect_in(path: Option<&OsStr>, home: &Path) -> Self {
        let Some(zstd) = path.and_then(|path| find_zstd(path, home)) else {
            return Self::Gzip;
        };
        let Ok(output) = Command::new(zstd).args(["--quiet", "--version"]).output() else {
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

fn find_zstd(path: &OsStr, home: &Path) -> Option<PathBuf> {
    let home = fs::canonicalize(home).unwrap_or_else(|_| home.to_path_buf());
    let restored: Vec<PathBuf> = HOME_PATHS.iter().map(|tools| home.join(tools)).collect();
    std::env::split_paths(path)
        .filter(|dir| dir.is_absolute() && is_safe_dir(dir, &restored))
        .map(|dir| dir.join("zstd"))
        .find(|zstd| is_safe_executable(zstd, &restored))
}

fn is_safe_dir(dir: &Path, restored: &[PathBuf]) -> bool {
    let Ok(real) = fs::canonicalize(dir) else {
        return false;
    };
    fs::metadata(&real).is_ok_and(|metadata| metadata.mode() & 0o002 == 0)
        && !restored.iter().any(|tools| real.starts_with(tools))
}

fn is_safe_executable(zstd: &Path, restored: &[PathBuf]) -> bool {
    let Ok(real) = fs::canonicalize(zstd) else {
        return false;
    };
    fs::metadata(&real).is_ok_and(|metadata| metadata.is_file() && metadata.mode() & 0o111 != 0)
        && !restored.iter().any(|tools| real.starts_with(tools))
}

#[must_use]
pub fn cache_version(paths: &[&str], compression: Compression) -> String {
    let mut components: Vec<&str> = paths.to_vec();
    components.push(compression.name());
    components.push(SALT);
    hex(&Sha256::digest(components.join("|").as_bytes()))
}
