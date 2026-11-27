// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fmt;
use std::num::NonZeroU64;

use super::{GUEST_SHARE, JIT_DIRECTORY};

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
    jit_name: String,
    guest_jit_file: String,
    guest_work_dir: String,
    guest_stage_dir: String,
}

impl JobPaths {
    pub(crate) fn of(reservation: ReservationId) -> Self {
        Self {
            jit_name: format!("{reservation}.jit"),
            guest_jit_file: format!("{GUEST_SHARE}/{JIT_DIRECTORY}/{reservation}.jit"),
            guest_work_dir: format!("{GUEST_GEN}/work/{reservation}"),
            guest_stage_dir: format!("{GUEST_GEN}/stage/{reservation}"),
        }
    }

    pub(crate) fn jit_name(&self) -> &str {
        &self.jit_name
    }

    pub(crate) fn guest_jit_file(&self) -> &str {
        &self.guest_jit_file
    }

    pub(crate) fn guest_work_dir(&self) -> &str {
        &self.guest_work_dir
    }

    pub(crate) fn guest_stage_dir(&self) -> &str {
        &self.guest_stage_dir
    }
}
