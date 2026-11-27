// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use anyhow::{Result, ensure};
use ring::digest::{SHA256, digest};
use velnor_cache_rules::{Access, Event, Scope, access_for};

use super::facts::{PullRequestHead, TrustedEvent, TrustedFacts};
use super::location::server_identity;
use super::proto::Denied;

pub(crate) const DATA: &str = "data.tar";
pub(crate) const KEY: &str = "key";
pub(crate) const VERSION: &str = "version";
pub(crate) const SCOPE: &str = "scope";
pub(crate) const MARKER: &str = "complete";
pub(crate) const ACCESSED: &str = "accessed";
pub(crate) const MAX_DATA_BYTES: u64 = 10 * 1024 * 1024 * 1024;
pub(crate) const MAX_KEY_LENGTH: usize = 512;

pub(crate) fn key_units(key: &str) -> usize {
    key.encode_utf16().count()
}

pub(crate) fn sha256_hex(text: &str) -> String {
    digest(&SHA256, text.as_bytes()).as_ref().iter().fold(
        String::with_capacity(64),
        |mut out, byte| {
            let _ = write!(out, "{byte:02x}");
            out
        },
    )
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct CacheOwner(String);

impl CacheOwner {
    pub(crate) fn of(facts: &TrustedFacts) -> Self {
        Self(format!(
            "{}\0{}",
            server_identity(facts.server_url.url()),
            facts.repository_id
        ))
    }
}

pub(super) fn owner_hash(owner: &CacheOwner) -> String {
    sha256_hex(&owner.0)
}

pub(super) fn scope_hash(owner: &CacheOwner, scope: &Scope) -> String {
    sha256_hex(&format!("{}\0{}", owner.0, scope.0))
}

pub(super) fn owner_parts(owner: &CacheOwner) -> [String; 3] {
    ["cache".to_owned(), "v1".to_owned(), owner_hash(owner)]
}

pub(super) fn scope_parts(owner: &CacheOwner, scope: &Scope) -> [String; 4] {
    let [cache, version, owner_hash] = owner_parts(owner);
    [cache, version, owner_hash, scope_hash(owner, scope)]
}

pub(super) fn entry_name(version: &str, key: &str) -> String {
    sha256_hex(&format!("{version}\n{key}"))
}

pub(crate) fn scope_dir(root: &Path, owner: &CacheOwner, scope: &Scope) -> PathBuf {
    scope_parts(owner, scope)
        .iter()
        .fold(root.to_path_buf(), |path, part| path.join(part))
}

pub(crate) fn entry_dir(
    root: &Path,
    owner: &CacheOwner,
    scope: &Scope,
    version: &str,
    key: &str,
) -> PathBuf {
    scope_dir(root, owner, scope).join(entry_name(version, key))
}

impl TrustedFacts {
    pub(crate) fn cache_access(&self) -> Access {
        let event = match &self.event {
            TrustedEvent::Branch { branch } => Event::Push {
                branch: branch.clone(),
            },
            TrustedEvent::Tag { name } => Event::Tag { name: name.clone() },
            TrustedEvent::MergeGroup { queue_branch } => Event::MergeGroup {
                queue_branch: queue_branch.clone(),
            },
            TrustedEvent::PullRequest {
                number,
                base_branch,
                ..
            } => Event::PullRequest {
                number: *number,
                base_branch: base_branch.clone(),
            },
        };
        access_for(&event, &self.default_branch)
    }

    pub(crate) fn cache_write_scope(&self) -> Result<Scope, Denied> {
        if !self.may_write_stores() {
            return Err(match self.event {
                TrustedEvent::PullRequest {
                    head: PullRequestHead::Fork,
                    ..
                } => Denied::ForkPullRequest,
                _ => Denied::ReadOnlyEvent,
            });
        }
        Ok(self.cache_access().write)
    }
}

pub(super) fn check_request(key: &str, version: &str) -> Result<()> {
    ensure!(
        !key.is_empty() && key_units(key) <= MAX_KEY_LENGTH,
        "the cache key must have 1 to {MAX_KEY_LENGTH} UTF-16 code units"
    );
    ensure!(
        version.len() == 64
            && version
                .bytes()
                .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f')),
        "the cache version must be a lowercase sha256 hex digest"
    );
    Ok(())
}
