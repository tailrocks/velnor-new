//! Focused producer and evidence gate cases.

use super::*;

#[test]
fn save_requires_verified_proof_gate() {
    let mut job = producer_job();
    let save = job
        .steps
        .iter_mut()
        .find(|step| {
            step.id
                .as_ref()
                .is_some_and(|value| value.as_str() == SAVE_ID)
        })
        .expect("source save");
    save.condition = Some(velnor_actions_contract::workflow::ir::CACHE_SAVE_CONDITION.to_owned());
    assert_rejected(job, "source_producer_mixed_computation");
    let mut bypass = producer_job();
    let save = bypass
        .steps
        .iter_mut()
        .find(|step| {
            step.id
                .as_ref()
                .is_some_and(|value| value.as_str() == SAVE_ID)
        })
        .expect("source save");
    save.condition = Some(format!("{} || true", metadata().save_condition()));
    assert_rejected(bypass, "source_producer_mixed_computation");
}
