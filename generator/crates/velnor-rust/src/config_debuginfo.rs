// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::error::Reason;
use velnor_model::CiDebugInfo;

pub(crate) fn debuginfo(
    configured: Option<&str>,
    default: CiDebugInfo,
) -> Result<CiDebugInfo, Reason> {
    match configured {
        None => Ok(default),
        Some("full") => Ok(CiDebugInfo::Full),
        Some("line-tables-only") => Ok(CiDebugInfo::LineTablesOnly),
        Some("none") => Ok(CiDebugInfo::None),
        Some(other) => Err(Reason::InvalidValue {
            key: "ci_debuginfo",
            value: other.to_owned(),
        }),
    }
}
