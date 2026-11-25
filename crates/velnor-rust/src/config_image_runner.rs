// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_model::RunnerLabel;

use crate::config_fork::expression_safe;
use crate::config_image::invalid;
use crate::error::Reason;

pub(crate) fn label(raw: Option<String>, key: &'static str) -> Result<Option<RunnerLabel>, Reason> {
    raw.map(|text| {
        RunnerLabel::new(text.clone())
            .ok()
            .filter(expression_safe)
            .ok_or_else(|| invalid(key, &text))
    })
    .transpose()
}
