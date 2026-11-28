// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde_json::{Value, json};

use super::checks::{acquire_ids, assert_clean};
use super::events::Event;
use super::fixtures::{TestResult, WORKFLOW, push_offer};
use super::harness::World;

fn deleted(events: &[Event], status: &str) -> bool {
    events
        .iter()
        .any(|event| event.label == "message.delete" && event.field("status") == status)
}

fn started(world: &mut World, test: &str, refusals: &Value) -> TestResult<()> {
    world.queue_message(91, &[push_offer(1, 9101, "main", WORKFLOW)])?;
    world.script_refusals(refusals)?;
    world.spawn_controller(test, &[])
}

fn stopped(mut world: World) -> TestResult<()> {
    world.stop()?;
    assert_eq!(world.finish(30)?, "Ok(Shutdown)");
    assert_clean(&world, "1");
    Ok(())
}

#[test]
fn rust_runner_controller_refreshes_after_a_401_acquire() -> TestResult<()> {
    if let Some(root) = super::child_root() {
        return super::child::run(&root);
    }
    let mut world = World::new("T40", 1, true)?;
    started(
        &mut world,
        "session_401::rust_runner_controller_refreshes_after_a_401_acquire",
        &json!({"acquire.expired": {"statuses": [401], "repeat_last": false}}),
    )?;
    world.wait_for_event("the delete of message 91", 60, |events| {
        deleted(events, "204")
    });
    let acquires = world.gh("acquire");
    assert_eq!(acquire_ids(&world), ["1", "1"]);
    assert_eq!(acquires[0].field("status"), "401");
    assert_eq!(acquires[1].field("status"), "200");
    assert!(
        world
            .gh("session.refresh")
            .iter()
            .any(|refresh| acquires[0].seq < refresh.seq && refresh.seq < acquires[1].seq),
        "the second acquire was not preceded by a session refresh"
    );
    let jits = world.gh("jit");
    assert_eq!(jits.len(), 1);
    assert_eq!(
        world
            .events()
            .iter()
            .filter(|event| event.is_runner_start())
            .count(),
        1
    );
    assert!(
        world
            .gh("message.delete")
            .iter()
            .all(|delete| delete.seq > jits[0].seq),
        "message 91 was deleted before its job was acquired"
    );
    stopped(world)
}

#[test]
fn rust_runner_controller_does_not_loop_on_a_repeated_401_acquire() -> TestResult<()> {
    if let Some(root) = super::child_root() {
        return super::child::run(&root);
    }
    let mut world = World::new("T41", 1, true)?;
    started(
        &mut world,
        "session_401::rust_runner_controller_does_not_loop_on_a_repeated_401_acquire",
        &json!({"acquire.expired": {"statuses": [401], "repeat_last": true}}),
    )?;
    world.wait_for_event("the delete of the redelivered message 91", 60, |events| {
        deleted(events, "204")
    });
    assert_eq!(acquire_ids(&world), ["1", "1"]);
    assert_eq!(world.gh("session.refresh").len(), 1);
    assert!(world.gh("jit").is_empty(), "a refused acquire got a jit");
    assert!(
        world
            .child_stderr()
            .lines()
            .any(|line| line
                .starts_with("rust_runner_job=cleaned reservation=2 exit=acquire_failed")),
        "{}",
        world.child_stderr()
    );
    stopped(world)
}

#[test]
fn rust_runner_controller_refreshes_after_a_401_acknowledge() -> TestResult<()> {
    if let Some(root) = super::child_root() {
        return super::child::run(&root);
    }
    let mut world = World::new("T42", 1, true)?;
    started(
        &mut world,
        "session_401::rust_runner_controller_refreshes_after_a_401_acknowledge",
        &json!({"message.expired": {"statuses": [401], "repeat_last": false}}),
    )?;
    world.wait_for_event("the retried delete of message 91", 60, |events| {
        deleted(events, "204")
    });
    let deletes = world.gh("message.delete");
    assert_eq!(deletes.len(), 2);
    assert_eq!(deletes[0].field("status"), "401");
    let refreshes = world.gh("session.refresh");
    assert_eq!(refreshes.len(), 1);
    assert!(deletes[0].seq < refreshes[0].seq && refreshes[0].seq < deletes[1].seq);
    assert_eq!(world.gh("jit").len(), 1);
    stopped(world)
}
