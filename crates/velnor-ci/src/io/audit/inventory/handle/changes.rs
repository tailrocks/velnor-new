// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::time::{Duration, SystemTime};

use nix::sys::stat::FileStat;

use super::Stamp;
use crate::io::restore_record::Record;

const TICK: Duration = Duration::from_millis(20);

impl Stamp {
    pub(in crate::io::audit::inventory) fn latest(&self, record: &Record) -> SystemTime {
        let vouched = self.vouched(record);
        counted(Self::time(self.modified), vouched).max(counted(Self::time(self.changed), vouched))
    }

    pub(in crate::io::audit::inventory) fn changed_unless_explained(
        &self,
        record: &Record,
        excluded: &[Stamp],
    ) -> SystemTime {
        let changed = Self::time(self.changed);
        let explained = self.modified == self.changed
            && excluded
                .iter()
                .any(|target| Self::time(target.modified.max(target.changed)) + TICK >= changed);
        counted(changed, explained || self.vouched(record))
    }

    fn vouched(&self, record: &Record) -> bool {
        match (
            u64::try_from(self.device),
            u64::try_from(self.inode),
            i64::try_from(self.changed.0),
            i64::try_from(self.changed.1),
        ) {
            (Ok(device), Ok(inode), Ok(seconds), Ok(nanos)) => {
                record.vouches((device, inode), (seconds, nanos))
            }
            _ => false,
        }
    }

    fn time((seconds, nanos): (i128, i128)) -> SystemTime {
        instant(
            i64::try_from(seconds).unwrap_or(0),
            i64::try_from(nanos).unwrap_or(0),
        )
    }
}

pub(super) fn instant(seconds: i64, nanos: i64) -> SystemTime {
    let nanos = Duration::from_nanos(u64::try_from(nanos).unwrap_or(0));
    match u64::try_from(seconds) {
        Ok(seconds) => SystemTime::UNIX_EPOCH + Duration::from_secs(seconds) + nanos,
        Err(_) => SystemTime::UNIX_EPOCH - Duration::from_secs(seconds.unsigned_abs()) + nanos,
    }
}

pub(super) fn counted(time: SystemTime, vouched: bool) -> SystemTime {
    if vouched {
        SystemTime::UNIX_EPOCH
    } else {
        time
    }
}

pub(super) fn vouched(stat: &FileStat, record: &Record) -> bool {
    match (
        u64::try_from(i128::from(stat.st_dev)),
        u64::try_from(i128::from(stat.st_ino)),
    ) {
        (Ok(device), Ok(inode)) => {
            record.vouches((device, inode), (stat.st_ctime, stat.st_ctime_nsec))
        }
        _ => false,
    }
}

pub(super) fn file_changed(stat: &FileStat, record: &Record) -> SystemTime {
    instant(stat.st_mtime, stat.st_mtime_nsec).max(counted(
        instant(stat.st_ctime, stat.st_ctime_nsec),
        vouched(stat, record),
    ))
}

pub(super) fn link_changed(stat: &FileStat, record: &Record) -> SystemTime {
    let vouched = vouched(stat, record);
    counted(instant(stat.st_mtime, stat.st_mtime_nsec), vouched)
        .max(counted(instant(stat.st_ctime, stat.st_ctime_nsec), vouched))
}

pub(super) fn same_link(first: &FileStat, second: &FileStat) -> bool {
    (first.st_dev, first.st_ino, first.st_mode) == (second.st_dev, second.st_ino, second.st_mode)
        && (first.st_ctime, first.st_ctime_nsec) == (second.st_ctime, second.st_ctime_nsec)
        && (first.st_mtime, first.st_mtime_nsec) == (second.st_mtime, second.st_mtime_nsec)
}
