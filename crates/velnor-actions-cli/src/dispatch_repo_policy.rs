// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::path::{Path, PathBuf};
use std::process::ExitCode;

pub(crate) fn gate_root() -> Option<PathBuf> {
    velnor_freshness::request::gate_root()
}

pub(crate) fn run(root: &Path) -> ExitCode {
    velnor_freshness::request::run(root, "velnor-actions")
}
