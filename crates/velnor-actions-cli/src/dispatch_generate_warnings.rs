// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

pub(super) fn warning_lines(warnings: &[String]) -> impl Iterator<Item = String> + '_ {
    warnings
        .iter()
        .map(|warning| format!("velnor-actions: WARNING: {warning}"))
}
