// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::error::Reason;

pub(crate) fn table(bytes: &[u8]) -> Result<toml::Table, Reason> {
    let text = std::str::from_utf8(bytes).map_err(|_| Reason::NotUtf8)?;
    toml::from_str(text).map_err(|error| Reason::Syntax(error.message().to_owned()))
}
