// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0
// SPDX-FileContributor: tailrocks/velnor-legacy@1b9fc0388925d21ff44ba1be5027684c38c8e4c9 crates/velnor-runner/src/git_mirror.rs

use std::fmt::Write;
use std::path::{Path, PathBuf};

use anyhow::{Result, bail};
use url::Url;

use super::facts::TrustedFacts;

const MAX_SERVER_KEY: usize = 200;

fn escaped(text: &str) -> String {
    let mut key = String::with_capacity(text.len());
    for byte in text.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.') {
            key.push(char::from(byte));
        } else {
            let _ = write!(key, "%{byte:02X}");
        }
    }
    key
}

fn server_key(url: &Url) -> Result<String> {
    let mut identity = format!("{}://{}", url.scheme(), url.host_str().unwrap_or_default());
    if let Some(port) = url.port() {
        let _ = write!(identity, ":{port}");
    }
    identity.push_str(url.path().trim_end_matches('/'));
    let key = escaped(&identity);
    if key.len() > MAX_SERVER_KEY {
        bail!("mirror: server identity '{identity}' is too long for a mirror directory");
    }
    Ok(key)
}

pub(crate) fn mirror_path(root: &Path, facts: &TrustedFacts) -> Result<PathBuf> {
    let (owner, name) = facts.repository.owner_and_name();
    let mut path = root.join("git");
    path.push(server_key(facts.server_url.url())?);
    for part in [owner, name] {
        path.push(part.to_ascii_lowercase());
    }
    path.push(format!("{}.git", facts.repository_id));
    Ok(path)
}
