// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_actions_mise::{PinnedTool, ToolCatalog};

pub(super) fn extend(pins: &mut Vec<String>, catalog: &ToolCatalog) {
    for row in include_str!("tool_archive_pins.tsv").lines() {
        let mut fields = row.split_whitespace();
        let tool = match fields.next() {
            Some("shellcheck") => PinnedTool::Shellcheck,
            Some("gh") => PinnedTool::Gh,
            _ => continue,
        };
        if fields.next() == Some(catalog.version(tool)) {
            pins.push(row.to_owned());
        }
    }
}
