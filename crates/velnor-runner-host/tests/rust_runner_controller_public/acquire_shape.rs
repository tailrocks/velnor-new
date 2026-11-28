// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::checks::{acquire_ids, assert_clean, deleted_messages};
use super::events::wait_for;
use super::fixtures::{TestResult, WORKFLOW, push_offer, with_acquire_returns};
use super::harness::World;

#[test]
fn rust_runner_controller_holds_an_acquire_that_repeats_the_request() -> TestResult<()> {
    held(
        "rust_runner_controller_holds_an_acquire_that_repeats_the_request",
        "duplicate",
    )
}

#[test]
fn rust_runner_controller_holds_an_acquire_that_adds_another_request() -> TestResult<()> {
    held(
        "rust_runner_controller_holds_an_acquire_that_adds_another_request",
        "extra",
    )
}

fn held(name: &str, shape: &str) -> TestResult<()> {
    if let Some(root) = super::child_root() {
        return super::child::run(&root);
    }
    let test = format!("acquire_shape::{name}");
    let mut world = World::new("T9", 1, true)?;
    world.queue_message(
        91,
        &[with_acquire_returns(
            push_offer(1, 9001, "main", WORKFLOW),
            shape,
        )],
    )?;
    world.spawn_controller(&test, &[])?;
    wait_for("the release of the slot of the held offer", 40, || {
        world
            .child_stderr()
            .lines()
            .any(|line| line.starts_with("rust_runner_job=cleaned reservation=1 "))
    });
    assert!(
        world.gh("jit").is_empty(),
        "a runner was registered for an acquire that was not exactly the request"
    );
    assert!(
        world.events().iter().all(|event| !event.is_runner_start()),
        "a runner was started for an acquire that was not exactly the request"
    );
    assert!(
        !deleted_messages(&world).contains(&"91".to_owned()),
        "the message of an uncertain acquire was acknowledged"
    );
    assert_eq!(acquire_ids(&world), ["1"]);
    world.stop()?;
    assert_eq!(world.finish()?, "Ok(Shutdown)");
    assert_clean(&world, "1");
    Ok(())
}
