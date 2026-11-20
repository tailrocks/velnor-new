// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MbxMode {
    Default,
    Required,
    Off,
}

impl MbxMode {
    #[must_use]
    pub const fn routed(self) -> bool {
        !matches!(self, Self::Off)
    }

    #[must_use]
    pub const fn written(self) -> bool {
        !matches!(self, Self::Default)
    }
}
