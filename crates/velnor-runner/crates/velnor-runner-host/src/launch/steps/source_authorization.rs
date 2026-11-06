// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::source;
use crate::scale_set::EnsureError;

pub(super) fn source_blocked(block: source::SourceBlock) -> EnsureError {
    eprintln!("source_admission=blocked reason={}", block.code());
    EnsureError::SourceBlocked {
        reason: block.code(),
    }
}
