// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use super::validate::{sha256_hex, validate_link_target, validate_relative_path, validate_sha256};
use crate::OrchestratorError;
use crate::internal::internal;

const NANOS_PER_SECOND: u32 = 1_000_000_000;

pub(super) const HEX_DIGITS: &[u8; 16] = b"0123456789abcdef";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryKind {
    File,
    Directory,
    Symlink,
}

impl EntryKind {
    pub fn parse(value: &str) -> Result<Self, OrchestratorError> {
        match value {
            "file" => Ok(Self::File),
            "directory" => Ok(Self::Directory),
            "symlink" => Ok(Self::Symlink),
            _ => Err(internal("projection_kind_unknown")),
        }
    }

    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::File => "file",
            Self::Directory => "directory",
            Self::Symlink => "symlink",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RealMtime {
    pub seconds: i64,
    pub nanoseconds: u32,
}

impl RealMtime {
    pub fn validate(&self) -> Result<(), OrchestratorError> {
        if self.nanoseconds >= NANOS_PER_SECOND {
            return Err(internal("projection_mtime_invalid"));
        }
        Ok(())
    }

    pub fn system_time(&self) -> Result<SystemTime, OrchestratorError> {
        self.validate()?;
        let total =
            i128::from(self.seconds) * i128::from(NANOS_PER_SECOND) + i128::from(self.nanoseconds);
        let magnitude = total.unsigned_abs();
        let seconds = u64::try_from(magnitude / 1_000_000_000)
            .map_err(|_| internal("projection_mtime_range"))?;
        let nanos = u32::try_from(magnitude % 1_000_000_000)
            .map_err(|_| internal("projection_mtime_range"))?;
        let duration = Duration::new(seconds, nanos);
        if total < 0 {
            UNIX_EPOCH
                .checked_sub(duration)
                .ok_or_else(|| internal("projection_mtime_range"))
        } else {
            UNIX_EPOCH
                .checked_add(duration)
                .ok_or_else(|| internal("projection_mtime_range"))
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectionEntry {
    pub path: String,
    pub kind: EntryKind,
    pub mode: u32,
    pub content_sha256: String,
    pub link_target: Option<String>,
    pub mtime: Option<RealMtime>,
}

impl ProjectionEntry {
    pub fn validate(&self) -> Result<(), OrchestratorError> {
        validate_relative_path(&self.path)?;
        if self.mode > 0o7777 {
            return Err(internal("projection_mode_invalid"));
        }
        validate_sha256(&self.content_sha256)?;
        if self.kind == EntryKind::Directory && self.content_sha256 != sha256_hex(&[]) {
            return Err(internal("projection_directory_digest"));
        }
        match self.kind {
            EntryKind::Symlink => {
                let Some(target) = self.link_target.as_deref() else {
                    return Err(internal("projection_link_missing"));
                };
                validate_link_target(target)?;
            }
            EntryKind::File | EntryKind::Directory => {
                if self.link_target.is_some() {
                    return Err(internal("projection_link_unexpected"));
                }
            }
        }
        if let Some(mtime) = &self.mtime {
            mtime.validate()?;
        }
        Ok(())
    }

    #[must_use]
    pub fn semantic_matches(&self, other: &Self) -> bool {
        self.path == other.path
            && self.kind == other.kind
            && self.mode == other.mode
            && self.content_sha256 == other.content_sha256
            && self.link_target == other.link_target
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CheckoutState {
    Unchanged,
    Changed,
    Missing,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RestoreDecision {
    Restore,
    KeepCurrent,
    Discard,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstallOutcome {
    pub mtime_restored: bool,
}
