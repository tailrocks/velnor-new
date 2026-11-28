// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::path::PathBuf;

const DEFAULT_ROOT: &str = "/var/lib/velnor";

pub(crate) fn store_root() -> PathBuf {
    std::env::var_os("VELNOR_STORE_ROOT")
        .filter(|root| !root.is_empty())
        .map_or_else(|| PathBuf::from(DEFAULT_ROOT), PathBuf::from)
}
