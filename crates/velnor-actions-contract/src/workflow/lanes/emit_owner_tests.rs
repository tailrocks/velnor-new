use super::*;
use cache_matrix::*;
use support::*;

#[path = "emit_owner_tests/cache_matrix.rs"]
mod cache_matrix;
#[path = "emit_owner_tests/support.rs"]
mod support;

#[test]
fn cache_reference_retarget_preserves_literal_strings_and_task_execution() {
    let mut original = owner_job(SOURCE_OWNER);
    let bind = bind_id(SOURCE_OWNER);
    let hosted_bind = bind_id(HOSTED_OWNER);
    let task = task_step(&bind);
    let task_kind = task.kind.clone();
    original.steps.extend([
        task,
        literal_shell_step(&bind),
        literal_action_step(&bind),
        literal_internal_step(&bind),
    ]);

    let retargeted = expand_owner(&original, HOSTED_OWNER, EmitLane::Hosted);
    assert_eq!(retargeted.steps[3].kind, task_kind);
    assert_literal_shell(&retargeted.steps[4], &bind, &hosted_bind);
    assert_literal_action(&retargeted.steps[5], &bind, &hosted_bind);
    assert_literal_internal(&retargeted.steps[6], &bind, &hosted_bind);
}

#[test]
fn every_typed_cache_layer_reference_rebinds_and_unknown_cache_reference_fails() {
    let mut original = owner_job(SOURCE_OWNER);
    let layers = cache_layers();
    let stages = cache_stages();
    append_cache_reference_steps(&mut original, &layers, &stages);

    let retargeted = expand_owner(&original, HOSTED_OWNER, EmitLane::Hosted);
    assert_cache_references_retargeted(&retargeted, layers.len(), stages.len());
    assert_unknown_cache_reference_fails();
}
