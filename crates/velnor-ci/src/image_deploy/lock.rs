// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::error::CiError;
use crate::image_candidate::hashing::sha256_hex;
use serde::Deserialize;

const MAX_DEBS: usize = 8;
pub const LIB_DIRECTORY: &str = "usr/lib/x86_64-linux-gnu";
pub const INCLUDE_DIRECTORY: &str = "usr/include";
pub const ARCH_HEADERS: &str = "usr/include/x86_64-linux-gnu/openssl";
pub const HEADERS: &str = "usr/include/openssl";

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Deb {
    pub package: String,
    pub url: String,
    pub sha256: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Lock {
    version: u32,
    #[serde(default)]
    deb: Vec<Deb>,
}

fn plain(text: &str) -> bool {
    !text.is_empty()
        && text
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'+' | b'-'))
}

pub fn parse_lock(text: &str) -> Result<Vec<Deb>, CiError> {
    let lock: Lock = toml::from_str(text).map_err(|error| {
        CiError::new(format!("the sysroot lock is invalid: {}", error.message()))
    })?;
    if lock.version != 1 {
        return Err(CiError::new(format!(
            "the sysroot lock has version {}, and this velnor-ci reads version 1",
            lock.version
        )));
    }
    if lock.deb.is_empty() || lock.deb.len() > MAX_DEBS {
        return Err(CiError::new(format!(
            "the sysroot lock lists {} packages, and it must list 1 to {MAX_DEBS}",
            lock.deb.len()
        )));
    }
    for (index, deb) in lock.deb.iter().enumerate() {
        let url_ok = deb.url.strip_prefix("https://").is_some_and(|rest| {
            std::path::Path::new(rest)
                .extension()
                .is_some_and(|extension| extension == "deb")
                && rest.split_once('/').is_some()
                && !rest.contains(['?', '#', '\\', ' '])
        });
        let hash_ok = deb.sha256.len() == 64
            && deb
                .sha256
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte));
        if !plain(&deb.package) || !url_ok || !hash_ok {
            return Err(CiError::new(format!(
                "package {} of the sysroot lock needs a plain name, an https URL of a .deb and a lowercase hex sha256",
                index + 1
            )));
        }
        if lock.deb[..index]
            .iter()
            .any(|other| other.package == deb.package)
        {
            return Err(CiError::new(format!(
                "the sysroot lock lists {} twice",
                deb.package
            )));
        }
    }
    Ok(lock.deb)
}

pub fn verify(deb: &Deb, bytes: &[u8]) -> Result<(), CiError> {
    let actual = sha256_hex(bytes);
    if actual == deb.sha256 {
        Ok(())
    } else {
        Err(CiError::new(format!(
            "{} does not match the sha256 in the sysroot lock (locked {}, downloaded {actual})",
            deb.package, deb.sha256
        )))
    }
}
