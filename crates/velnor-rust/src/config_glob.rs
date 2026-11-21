// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::error::Reason;

pub(crate) fn unsupported_glob(key: &'static str, entry: &str) -> Result<(), Reason> {
    if entry.contains(['?', '[', ']']) {
        return Err(Reason::UnsupportedGlob {
            key,
            value: entry.to_owned(),
        });
    }
    Ok(())
}
