// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_actions_contract::{Step, StepKind, StepRole};

pub(crate) const WRITER_RECHECK_STEP_NAME: &str = "Recheck cache state after queue slot";

fn is_probe_role(role: Option<StepRole>) -> bool {
    matches!(
        role,
        Some(
            StepRole::ToolsCacheRestore
                | StepRole::CargoSourcesRestore
                | StepRole::MbxBundleRestore
                | StepRole::FamilyRestore
        )
    )
}

pub(crate) fn is_recoverable_probe(step: &Step) -> bool {
    is_probe_role(step.role) && matches!(step.kind, StepKind::Action { .. })
}

pub(crate) fn writer_recheck_step(base_steps: &[Step]) -> Option<Step> {
    base_steps
        .iter()
        .find(|step| is_recoverable_probe(step))
        .map(|probe| {
            let mut recheck = probe.clone();
            recheck.id = None;
            WRITER_RECHECK_STEP_NAME.clone_into(&mut recheck.name);
            recheck.condition = None;
            recheck
        })
}

/// Point the writer recheck at the reader branch key outputs.
///
/// The recheck runs before the writer's own key step, so a cloned probe
/// `with` that reads `steps.mbx-bundle-key.outputs.*` would reference a
/// step that has not run yet. The reader publishes the same computation as
/// job outputs, and the writer already needs the reader, so the recheck
/// restores by `needs.<reader>.outputs.*` instead. Other `with` entries
/// (paths, conditions) are left untouched.
pub(crate) fn rewrite_recheck_step_keys(recheck: &mut Step, reader_job: &str) {
    use velnor_actions_contract::workflow::cache_family_mbx::{
        MBX_BUNDLE_KEY_EXPR, MBX_BUNDLE_RESTORE_KEYS_EXPR, mbx_bundle_key_needs_expr,
        mbx_bundle_recheck_condition, mbx_bundle_restore_keys_needs_expr,
    };
    let StepKind::Action { with, .. } = &mut recheck.kind else {
        return;
    };
    let key_needs = mbx_bundle_key_needs_expr(reader_job);
    let restore_keys_needs = mbx_bundle_restore_keys_needs_expr(reader_job);
    let mut key_rewritten = false;
    for value in with.values_mut() {
        if *value == MBX_BUNDLE_KEY_EXPR {
            value.clone_from(&key_needs);
            key_rewritten = true;
        } else if *value == MBX_BUNDLE_RESTORE_KEYS_EXPR {
            value.clone_from(&restore_keys_needs);
        }
    }
    if key_rewritten {
        recheck.condition = Some(mbx_bundle_recheck_condition(reader_job));
    }
}
