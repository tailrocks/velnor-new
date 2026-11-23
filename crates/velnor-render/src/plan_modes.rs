// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_model::{Gate, GateNeed};

use crate::gate::{Setup, output, plan_read, plan_source};

/// The plan mode of every root the gate covers, as one expression, so that a step that cannot
/// evaluate the lane's `if` can still tell an unselected lane from a failed one.
pub(crate) fn plan_modes(gate: &Gate, setup: Setup) -> String {
    let source = plan_source(&gate.from);
    let reads: Vec<String> = gate
        .roots()
        .map(|slug| plan_read(&source, &output(slug, GateNeed::Selected), setup))
        .collect();
    if let [only] = reads.as_slice() {
        return format!("${{{{ {only} }}}}");
    }
    let holes: Vec<String> = (0..reads.len()).map(|at| format!("{{{at}}}")).collect();
    let args: Vec<String> = reads.iter().map(|read| format!("({read})")).collect();
    format!(
        "${{{{ format('{}', {}) }}}}",
        holes.join(" "),
        args.join(", ")
    )
}
