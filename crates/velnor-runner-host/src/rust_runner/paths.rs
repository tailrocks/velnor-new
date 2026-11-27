// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fmt;
use std::num::NonZeroU64;

use super::{GUEST_SHARE, TRUSTED_DIRECTORY};

pub(crate) const GUEST_GEN: &str = "/srv/velnor/gen";

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
    trusted_name: String,
    guest_trusted_file: String,
    guest_work_dir: String,
}

impl JobPaths {
    pub(crate) fn of(reservation: ReservationId) -> Self {
        Self {
            trusted_name: format!("{reservation}.json"),
            guest_trusted_file: format!("{GUEST_SHARE}/{TRUSTED_DIRECTORY}/{reservation}.json"),
            guest_work_dir: format!("{GUEST_GEN}/work/{reservation}"),
        }
    }

    pub(crate) fn trusted_name(&self) -> &str {
        &self.trusted_name
    }

    pub(crate) fn guest_trusted_file(&self) -> &str {
        &self.guest_trusted_file
    }

    pub(crate) fn guest_work_dir(&self) -> &str {
        &self.guest_work_dir
    }
}
