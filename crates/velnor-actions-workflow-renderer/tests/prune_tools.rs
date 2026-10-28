// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_actions_workflow_renderer::cache_prune::{
    TOOLS_PRUNE_PREFIX, TOOLS_PRUNE_UNIT, TOOLS_RETAINED_ENTRIES, prune_group_key, prune_script,
    prune_targets,
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
fn tools_keys_share_one_prune_group() {
    for key in [
        "mise-tools-v2-33b8bde4f201ea4312115e8b97c50876eaafa5398f6fe2ce130af30459195fb",
        "mise-tools-v2-0000000000000000000000000000000000000000000000000000000000000000",
    ] {
        assert_eq!(prune_group_key(key), TOOLS_PRUNE_UNIT, "key: {key}");
    }
}

#[test]
fn snapshot_keys_group_by_unit_without_digest() {
    assert_eq!(
        prune_group_key(
            "velnor-v2-sources-trusted-s256-e32b46924cd679f385a2d1a7e3c22253aa3c0be67fb101bb0e636963ba16b528"
        ),
        "velnor-v2-sources-trusted"
    );
    assert_eq!(
        prune_group_key(
            "velnor-v1-state-trusted-s256-0d58c66670730fc3af7f9579c097b5b947410057122dc7bddc2482c60f929886"
        ),
        "velnor-v1-state-trusted"
    );
}

#[test]
fn keys_without_snapshot_suffix_keep_identity() {
    assert_eq!(
        prune_group_key("velnor-v1-state-trusted"),
        "velnor-v1-state-trusted"
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
