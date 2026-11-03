// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_actions_workflow_renderer::cache_prune::{
    TOOLS_PRUNE_PREFIX, TOOLS_RETAINED_ENTRIES, prune_script, prune_targets,
};

#[test]
fn tools_family_has_a_retained_prune_target() {
    let targets = prune_targets();
    assert!(
        targets
            .iter()
            .any(|(prefix, keep)| prefix == TOOLS_PRUNE_PREFIX && *keep == TOOLS_RETAINED_ENTRIES),
        "unexpected prune targets: {targets:?}"
    );
}

#[test]
fn prune_script_emits_tools_retention() {
    let script = prune_script();
    let invocation = format!("prune '{TOOLS_PRUNE_PREFIX}' {TOOLS_RETAINED_ENTRIES}");
    assert!(
        script.contains(&invocation),
        "prune script is missing the tools retention call"
    );
}
