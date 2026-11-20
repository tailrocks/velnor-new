// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use crate::yaml::{Yaml, entry, text};

pub(crate) fn workflow_env(roots: &BTreeMap<String, String>) -> Yaml {
    Yaml::Map(
        [
            entry("MISE_AUTO_INSTALL", text("false")),
            entry("RUSTUP_AUTO_INSTALL", text("0")),
        ]
        .into_iter()
        .chain(roots.iter().map(|(name, value)| entry(name, text(value))))
        .collect(),
    )
}
