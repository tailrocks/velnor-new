// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use rustix::fs::Mode;
use rustix::process::umask;
use tar::Header;

use crate::width::widen;

#[derive(Clone, Copy, Debug)]
pub(crate) struct Umask(u32);

impl Umask {
    pub(crate) fn read() -> Self {
        let current = umask(Mode::empty());
        umask(current);
        Self(widen(current.bits()))
    }

    pub(super) fn restored_mode(self, header: &Header) -> u32 {
        let fallback = if header.entry_type().is_dir() {
            0o755
        } else {
            0o644
        };
        header.mode().unwrap_or(fallback) & 0o777 & !self.0
    }
}
