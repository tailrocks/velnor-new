// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde::Deserialize;

#[derive(Deserialize)]
pub(super) struct ListedConfig {
    isolated: Option<bool>,
    isolate_network: Option<bool>,
    forward_ssh_agent: Option<bool>,
    disk_limit_bytes: Option<u64>,
    #[serde(default)]
    mounts: Vec<serde_json::Value>,
}

impl ListedConfig {
    pub(super) fn matches(&self, disk_bytes: u64) -> bool {
        self.isolated == Some(true)
            && self.isolate_network == Some(true)
            && self.forward_ssh_agent == Some(false)
            && self.disk_limit_bytes == Some(disk_bytes)
            && self.mounts.is_empty()
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

pub(super) const GUEST_USER: &str = "velnor-job";
pub(super) const USER_HOME: &str = concat!("/home", "/velnor-job");
const NOLOGIN: &str = "/usr/sbin/nologin";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GuestUser {
    uid: u32,
    gid: u32,
    home: String,
}

impl GuestUser {
    #[must_use]
    pub const fn uid(&self) -> u32 {
        self.uid
    }

    #[must_use]
    pub const fn gid(&self) -> u32 {
        self.gid
    }

    #[must_use]
    pub fn home(&self) -> &str {
        &self.home
    }

    #[cfg(feature = "test-support")]
    #[must_use]
    pub fn for_tests(uid: u32, gid: u32, home: &str) -> Self {
        Self {
            uid,
            gid,
            home: home.to_owned(),
        }
    }

    pub(super) fn parse(output: &str) -> Option<Self> {
        let mut lines = output.lines();
        let line = lines.next()?;
        let mut fields = line.split(':');
        let name = fields.next()?;
        let _password = fields.next()?;
        let uid: u32 = fields.next()?.parse().ok()?;
        let gid: u32 = fields.next()?.parse().ok()?;
        let _gecos = fields.next()?;
        let home = fields.next()?;
        let shell = fields.next()?;
        (name == GUEST_USER
            && uid != 0
            && gid != 0
            && shell == NOLOGIN
            && home == USER_HOME
            && fields.next().is_none()
            && lines.next().is_none())
        .then(|| Self {
            uid,
            gid,
            home: home.to_owned(),
        })
    }
}
