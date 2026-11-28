// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::checks::{acquire_ids, assert_clean};
use super::events::wait_for;
use super::fixtures::{TestResult, WORKFLOW, assigned, push_offer, with_acquire_returns};
use super::harness::World;

#[test]
fn rust_runner_controller_goes_to_jit_for_an_assigned_offer() -> TestResult<()> {
    let offers = [with_acquire_returns(
        assigned(push_offer(1, 9901, "main", WORKFLOW)),
        "none",
    )];
    goes_to_jit(
        "rust_runner_controller_goes_to_jit_for_an_assigned_offer",
        &offers,
    )
}

#[test]
fn rust_runner_controller_goes_to_jit_for_an_assigned_offer_that_supersedes_an_available_one()
-> TestResult<()> {
    let available = with_acquire_returns(push_offer(1, 9901, "main", WORKFLOW), "none");
    let offers = [available.clone(), assigned(available)];
    goes_to_jit(
        "rust_runner_controller_goes_to_jit_for_an_assigned_offer_that_supersedes_an_available_one",
        &offers,
    )
}

fn goes_to_jit(name: &str, offers: &[serde_json::Value]) -> TestResult<()> {
    if let Some(root) = super::child_root() {
        return super::child::run(&root);
    }
    let test = format!("assigned::{name}");
    let mut world = World::new("T43", 1, true)?;
    world.queue_message(99, offers)?;
    world.spawn_controller(&test, &[])?;
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
