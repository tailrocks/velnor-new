// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_model::PathGlob;

use crate::error::Reason;

pub(crate) fn excludes(entries: Vec<String>) -> Result<Vec<PathGlob>, Reason> {
    let invalid = |value| Reason::InvalidValue {
        key: "exclude",
        value,
    };
    entries
        .into_iter()
        .map(|entry| PathGlob::new(entry.trim_end_matches('/')).map_err(|_| invalid(entry)))
        .collect()
}
