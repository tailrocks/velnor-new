// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{GenerationPreparation, push};

pub(super) fn file_lines(out: &mut String, prep: &GenerationPreparation) {
    if prep.config.dco.is_some() {
        push(
            out,
            &format!("  {}", velnor_actions_contract::DCO_CONFIG_PATH),
        );
    }
}
