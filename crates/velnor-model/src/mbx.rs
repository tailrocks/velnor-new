// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

/// Whether the generated jobs run Cargo through mbx. `Default` and `Required` render the same
/// jobs; the difference is only whether `[cache] mbx = true` was written, which a helper must
/// know how to read.
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
