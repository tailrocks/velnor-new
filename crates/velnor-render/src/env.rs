// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::yaml::{Yaml, entry, text};

pub(crate) fn workflow_env() -> Yaml {
    Yaml::Map(vec![
        entry("MISE_AUTO_INSTALL", text("false")),
        entry("RUSTUP_AUTO_INSTALL", text("0")),
    ])
}
