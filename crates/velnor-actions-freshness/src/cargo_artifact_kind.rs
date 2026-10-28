// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CargoArtifactKind {
    Bin,
    Example,
}

impl CargoArtifactKind {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Bin => "bin",
            Self::Example => "example",
        }
    }
}
