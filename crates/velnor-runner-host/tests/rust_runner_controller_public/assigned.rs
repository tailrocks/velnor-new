// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::checks::{acquire_ids, assert_clean};
use super::events::wait_for;
use super::fixtures::{TestResult, WORKFLOW, assigned, push_offer, with_acquire_returns};
use super::harness::World;

pub(super) fn goes_to_jit(test: &str) -> TestResult<()> {
    let offers = [with_acquire_returns(
        assigned(push_offer(1, 9901, "main", WORKFLOW)),
        "none",
    )];
    run(test, &offers)
}

pub(super) fn goes_to_jit_after_an_available_one(test: &str) -> TestResult<()> {
    let available = with_acquire_returns(push_offer(1, 9901, "main", WORKFLOW), "none");
    run(test, &[available.clone(), assigned(available)])
}

fn run(test: &str, offers: &[serde_json::Value]) -> TestResult<()> {
    let mut world = World::new("T43", 1, true)?;
    world.queue_message(99, offers)?;
    world.spawn_controller(test, &[])?;
    world.wait_for_event("an acquire or a jit", 40, |events| {
        events
            .iter()
            .any(|event| event.label == "acquire" || event.label == "jit")
    });
    assert!(
        acquire_ids(&world).is_empty(),
        "an assigned offer was acquired: {:?}",
        acquire_ids(&world)
    );
    world.wait_for_event("the delete of message 99", 40, |events| {
        events
            .iter()
            .any(|event| event.label == "message.delete" && event.field("id") == "99")
    });
    wait_for("the release of the slot of the job", 40, || {
        world
            .child_stderr()
            .lines()
            .any(|line| line.starts_with("rust_runner_job=cleaned reservation=1 "))
    });
    assert_eq!(acquire_ids(&world), Vec::<String>::new());
    let events = world.events();
    let jits = world.gh("jit");
    assert_eq!(jits.len(), 1, "no single jit for the assigned offer");
    assert_eq!(
        events
            .iter()
            .filter(|event| event.is_runner_start())
            .count(),
        1
    );
    let delete = world.gh("message.delete");
    assert!(
        delete[0].seq > jits[0].seq,
        "message 99 was acknowledged before the jit"
    );
    world.stop()?;
    assert_eq!(world.finish(30)?, "Ok(Shutdown)");
    assert_clean(&world, "1");
    Ok(())
}
