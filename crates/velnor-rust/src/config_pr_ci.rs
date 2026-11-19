// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_model::PrCi;

use crate::error::Reason;

pub(crate) fn parse(configured: Option<&str>, skip_draft: bool) -> Result<PrCi, Reason> {
    match configured {
        None | Some("always") => Ok(PrCi::Always),
        Some("label") if skip_draft => Err(Reason::PrCiWithSkipDraft),
        Some("label") => Ok(PrCi::Label),
        Some(other) => Err(Reason::InvalidValue {
            key: "pr_ci",
            value: other.to_owned(),
        }),
    }
}
