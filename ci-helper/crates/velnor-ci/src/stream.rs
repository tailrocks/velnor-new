// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

#[must_use]
pub const fn separate_group(has_terminal: bool) -> bool {
    !has_terminal
}
