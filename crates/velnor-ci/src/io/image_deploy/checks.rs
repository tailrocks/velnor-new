// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fmt::Write as _;
use std::fs;
use std::path::Path;

use crate::cli_image::{ImageGhcrArgs, ImageVerifyArgs};
use crate::error::CiError;
use crate::image_candidate::hashing::sha256_hex;
use crate::image_deploy::cpu::refuse_cpu_flags;
use crate::image_deploy::ghcr::preflight;
use crate::io::{api_url, http, read_limited, token};

const MAX_BINARY_BYTES: u64 = 1 << 30;

pub(super) fn ghcr_preflight(args: &ImageGhcrArgs) -> Result<String, CiError> {
    let github = http::HttpGitHub::new(api_url(), token()?, http::DEFAULT_TIMEOUT);
    preflight(&github, &args.repository)
}

fn expected(sums: &str) -> Result<Vec<(String, String)>, CiError> {
    let mut entries = Vec::new();
    for entry in sums.split(',').filter(|entry| !entry.trim().is_empty()) {
        match entry.trim().split_once('=') {
            Some((name, hash))
                if !name.is_empty()
                    && !name.contains(['/', '\\'])
                    && hash.len() == 64
                    && hash
                        .bytes()
                        .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)) =>
            {
                entries.push((name.to_owned(), hash.to_owned()));
            }
            _ => {
                return Err(CiError::new(format!("`{entry}` is not <binary>=<sha256>")));
            }
        }
    }
    if entries.is_empty() {
        return Err(CiError::new(
            "the build job reported no staged binaries, so there is nothing to verify",
        ));
    }
    Ok(entries)
}

fn read_regular(path: &Path) -> Result<Vec<u8>, CiError> {
    let meta = fs::symlink_metadata(path)
        .map_err(|error| CiError::new(format!("cannot inspect {}: {error}", path.display())))?;
    if !meta.is_file() || meta.len() > MAX_BINARY_BYTES {
        return Err(CiError::new(format!(
            "{} is not a regular file of a sane size",
            path.display()
        )));
    }
    let file = fs::File::open(path)
        .map_err(|error| CiError::new(format!("cannot read {}: {error}", path.display())))?;
    read_limited(file, MAX_BINARY_BYTES)
        .map_err(|error| CiError::new(format!("cannot read {}: {error}", path.display())))
}

pub(super) fn cpu_flags() -> Result<String, CiError> {
    refuse_cpu_flags(&std::env::vars().collect::<Vec<_>>())
}

pub(super) fn verify(args: &ImageVerifyArgs) -> Result<String, CiError> {
    verify_staged(&args.dir, &args.sums)
}

pub fn verify_staged(dir: &Path, sums: &str) -> Result<String, CiError> {
    let wanted = expected(sums)?;
    let entries = fs::read_dir(dir)
        .map_err(|error| CiError::new(format!("cannot read {}: {error}", dir.display())))?;
    for entry in entries {
        let entry = entry
            .map_err(|error| CiError::new(format!("cannot read {}: {error}", dir.display())))?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if !wanted.iter().any(|(known, _)| *known == name) {
            return Err(CiError::new(format!(
                "{} holds {name}, which the build job did not stage",
                dir.display()
            )));
        }
    }
    let mut report = String::new();
    for (name, hash) in &wanted {
        let found = sha256_hex(&read_regular(&dir.join(name))?);
        if found != *hash {
            return Err(CiError::new(format!(
                "{name} has sha256 {found}, and the build job staged {hash}; the artifact was changed after it was staged"
            )));
        }
        let _ = writeln!(report, "{name} matches the staged sha256 {hash}");
    }
    Ok(report)
}
