// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::path::Path;

use serde::Deserialize;

#[derive(Deserialize)]
struct ListedMount {
    source: String,
    destination: String,
}

#[derive(Deserialize)]
pub(super) struct ListedConfig {
    isolated: Option<bool>,
    isolate_network: Option<bool>,
    forward_ssh_agent: Option<bool>,
    disk_limit_bytes: Option<u64>,
    #[serde(default)]
    mounts: Vec<ListedMount>,
}

pub(super) struct Shape<'a> {
    pub(super) share: &'a Path,
    pub(super) guest_share: &'a str,
    pub(super) disk_bytes: u64,
}

impl ListedConfig {
    pub(super) fn matches(&self, shape: &Shape<'_>) -> bool {
        let mounted = match self.mounts.as_slice() {
            [mount] => {
                Path::new(&mount.source) == shape.share && mount.destination == shape.guest_share
            }
            _ => false,
        };
        self.isolated == Some(true)
            && self.isolate_network == Some(true)
            && self.forward_ssh_agent == Some(false)
            && self.disk_limit_bytes == Some(shape.disk_bytes)
            && mounted
    }
}

pub(super) fn orb_version(output: &str) -> Option<String> {
    let line = output
        .lines()
        .find_map(|line| line.strip_prefix("Version:"))?;
    let mut tokens = line.split_whitespace();
    let version = tokens.next()?;
    let build = tokens
        .next()
        .and_then(|token| token.strip_prefix('(')?.strip_suffix(')'));
    let joined = build.map_or_else(|| version.to_owned(), |build| format!("{version}-{build}"));
    joined
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || byte == b'.' || byte == b'-')
        .then_some(joined)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GuestUser {
    pub uid: u32,
    pub gid: u32,
    pub home: String,
}

impl GuestUser {
    pub(super) fn parse(output: &str) -> Option<Self> {
        let mut lines = output.lines();
        let uid: u32 = lines.next()?.trim().parse().ok()?;
        let gid: u32 = lines.next()?.trim().parse().ok()?;
        let home = lines.next()?.trim();
        let clean = home.starts_with('/') && home.bytes().all(|byte| byte.is_ascii_graphic());
        (uid != 0 && gid != 0 && clean && lines.next().is_none()).then(|| Self {
            uid,
            gid,
            home: home.to_owned(),
        })
    }
}
