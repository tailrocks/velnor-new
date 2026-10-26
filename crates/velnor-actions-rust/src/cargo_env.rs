// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::ffi::OsString;

use crate::tasks::TaskKind;

pub const RUSTDOCFLAGS_ENV: &str = "RUSTDOCFLAGS";
pub const DENY_WARNINGS: &str = "-D warnings";

#[must_use]
pub fn cargo_payload_env(kind: TaskKind) -> Vec<(OsString, OsString)> {
    if kind == TaskKind::Doc {
        vec![(
            OsString::from(RUSTDOCFLAGS_ENV),
            OsString::from(DENY_WARNINGS),
        )]
    } else {
        Vec::new()
    }
}
