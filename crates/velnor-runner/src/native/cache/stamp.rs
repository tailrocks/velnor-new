// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::ffi::OsStr;
use std::io::Read;
use std::os::fd::{AsFd, BorrowedFd};

use anyhow::{Context, Result};
use rustix::fs::{AtFlags, Timespec, Timestamps, UTIME_OMIT, futimens, utimensat};
use tar::Entry;

const NANOS_DIGITS: usize = 9;
const NANOS_PER_SECOND: i128 = 1_000_000_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Stamp {
    seconds: i64,
    nanos: u32,
}

impl Stamp {
    pub(super) fn of(seconds: i64, nanos: i128) -> Self {
        Self {
            seconds,
            nanos: u32::try_from(nanos).unwrap_or_default(),
        }
    }

    pub(super) fn pax(self) -> Option<String> {
        if self.seconds >= 0 {
            return (self.nanos != 0).then(|| format!("{}.{:09}", self.seconds, self.nanos));
        }
        let total = i128::from(self.seconds) * NANOS_PER_SECOND + i128::from(self.nanos);
        let magnitude = total.unsigned_abs();
        let sign = if total < 0 { "-" } else { "" };
        let per = NANOS_PER_SECOND.unsigned_abs();
        Some(format!("{sign}{}.{:09}", magnitude / per, magnitude % per))
    }

    fn parse(text: &str) -> Option<Self> {
        let (whole, fraction) = text.split_once('.').unwrap_or((text, ""));
        if !fraction.bytes().all(|byte| byte.is_ascii_digit()) {
            return None;
        }
        let digits: String = fraction
            .chars()
            .chain(std::iter::repeat('0'))
            .take(NANOS_DIGITS)
            .collect();
        let nanos: u32 = digits.parse().ok()?;
        let seconds: i64 = whole.parse().ok()?;
        if whole.starts_with('-') && nanos != 0 {
            return Some(Self {
                seconds: seconds.checked_sub(1)?,
                nanos: 1_000_000_000 - nanos,
            });
        }
        Some(Self { seconds, nanos })
    }

    pub(super) fn of_entry<R: Read>(entry: &mut Entry<'_, R>) -> Option<Self> {
        let whole = i64::try_from(entry.header().mtime().ok()?).ok()?;
        let precise = entry
            .pax_extensions()
            .ok()
            .flatten()
            .into_iter()
            .flatten()
            .flatten()
            .find(|extension| extension.key() == Ok("mtime"))
            .and_then(|extension| Self::parse(extension.value().ok()?));
        Some(precise.unwrap_or(Self {
            seconds: whole,
            nanos: 0,
        }))
    }

    fn timestamps(self) -> Timestamps {
        Timestamps {
            last_access: Timespec {
                tv_sec: 0,
                tv_nsec: UTIME_OMIT,
            },
            last_modification: Timespec {
                tv_sec: self.seconds,
                tv_nsec: i64::from(self.nanos),
            },
        }
    }

    pub(super) fn apply(self, target: impl AsFd) -> Result<()> {
        futimens(target, &self.timestamps()).context("set the modification time")
    }

    pub(super) fn apply_to_link(self, parent: BorrowedFd<'_>, name: &OsStr) -> Result<()> {
        utimensat(parent, name, &self.timestamps(), AtFlags::SYMLINK_NOFOLLOW)
            .context("set the modification time of a link")
    }
}
