// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

/// Which private save a repository-privacy recheck guards.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Publication {
    BuildDirectory,
    Sources,
}
