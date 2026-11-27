// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_model::Gate;

pub(crate) fn package_selection(gate: &Gate, package_args: &str) -> Option<String> {
    if gate.packages.is_empty() {
        return None;
    }
    let args = format!("format(' {{0}} ', {package_args})");
    let selected = gate
        .packages
        .iter()
        .map(|package| format!("contains({args}, ' -p {package} ')"))
        .chain([format!("contains({args}, ' --workspace ')")])
        .collect::<Vec<_>>()
        .join(" || ");
    Some(if gate.unselected {
        format!("!({selected})")
    } else {
        format!("({selected})")
    })
}
