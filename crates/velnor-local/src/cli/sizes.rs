// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use clap::Args;

use crate::pairs::{
    DEFAULT_HOME_BYTES, DEFAULT_TMP_BYTES, DEFAULT_WORK_BYTES, Sizes, parse_home_size,
    parse_tmp_size, parse_work_size,
};

#[derive(Clone, Debug, Default, Args)]
pub struct StorageArgs {
    #[arg(
        long,
        value_name = "SIZE",
        value_parser = parse_home_size,
        help = "Size cap of the runner's home volume, /home/runner, in pair mode (2g by default); a whole number with m or g, 1g to 64g. Plain mode has no tmpfs caps"
    )]
    pub home_size: Option<u64>,
    #[arg(
        long,
        value_name = "SIZE",
        value_parser = parse_work_size,
        help = "Size cap of the runner's work volume, /home/runner/_work, in pair mode (16g by default); 1g to 64g. A write past --runner-memory is OOM-killed before it reaches this cap"
    )]
    pub work_size: Option<u64>,
    #[arg(
        long,
        value_name = "SIZE",
        value_parser = parse_tmp_size,
        help = "Size cap of the runner's /tmp tmpfs in pair mode (2g by default); 1g to 64g"
    )]
    pub tmp_size: Option<u64>,
}

impl StorageArgs {
    #[must_use]
    pub fn given(&self) -> Vec<&'static str> {
        [
            ("--home-size", self.home_size),
            ("--work-size", self.work_size),
            ("--tmp-size", self.tmp_size),
        ]
        .into_iter()
        .filter_map(|(flag, size)| size.map(|_| flag))
        .collect()
    }

    #[must_use]
    pub fn storage(&self) -> Sizes {
        let defaults = Sizes::default();
        Sizes {
            home_bytes: self.home_size.unwrap_or(defaults.home_bytes),
            work_bytes: self.work_size.unwrap_or(defaults.work_bytes),
            tmp_bytes: self.tmp_size.unwrap_or(defaults.tmp_bytes),
        }
    }
}

#[derive(Debug, Args)]
pub struct SuperviseSizes {
    #[arg(long, default_value_t = DEFAULT_HOME_BYTES, help = "Size cap of each runner's home volume, in bytes")]
    pub home_bytes: u64,
    #[arg(long, default_value_t = DEFAULT_WORK_BYTES, help = "Size cap of each runner's work volume, in bytes")]
    pub work_bytes: u64,
    #[arg(long, default_value_t = DEFAULT_TMP_BYTES, help = "Size cap of each runner's /tmp tmpfs, in bytes")]
    pub tmp_bytes: u64,
}

impl SuperviseSizes {
    #[must_use]
    pub const fn storage(&self) -> Sizes {
        Sizes {
            home_bytes: self.home_bytes,
            work_bytes: self.work_bytes,
            tmp_bytes: self.tmp_bytes,
        }
    }
}
