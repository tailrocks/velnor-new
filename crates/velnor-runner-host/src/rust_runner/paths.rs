// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fmt;
use std::num::NonZeroU64;
use std::path::{Path, PathBuf};

use super::{GUEST_STORE, JIT_DIRECTORY};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ReservationId(NonZeroU64);

impl ReservationId {
    #[must_use]
    pub fn new(value: u64) -> Option<Self> {
        NonZeroU64::new(value).map(Self)
    }

    #[must_use]
    pub const fn get(self) -> u64 {
        self.0.get()
    }
}

impl fmt::Display for ReservationId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}", self.0)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct JobPaths {
    jit_file: PathBuf,
    guest_jit_file: String,
    guest_work_dir: String,
}

impl JobPaths {
    pub(crate) fn of(host_store: &Path, reservation: ReservationId) -> Self {
        Self {
            jit_file: host_store
                .join(JIT_DIRECTORY)
                .join(format!("{reservation}.jit")),
            guest_jit_file: format!("{GUEST_STORE}/{JIT_DIRECTORY}/{reservation}.jit"),
            guest_work_dir: format!("{GUEST_STORE}/work/{reservation}"),
        }
    }

    pub(crate) fn jit_file(&self) -> &Path {
        &self.jit_file
    }

    pub(crate) fn guest_jit_file(&self) -> &str {
        &self.guest_jit_file
    }

    pub(crate) fn guest_work_dir(&self) -> &str {
        &self.guest_work_dir
    }
}
