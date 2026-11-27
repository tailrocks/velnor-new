// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::os::unix::fs::PermissionsExt;

use velnor_runner_host::{Readiness, controller_readiness};

use super::checks::{acquire_ids, assert_clean, jit_name, reservation, runner_name};
use super::events::wait_for;
use super::fixtures::{TestResult, WORKFLOW, push_offer};
use super::harness::World;

const FAILING: &str = "FAKE_RUNNER_EXIT=1\n";
const FAILING_HELD: &str = "FAKE_RUNNER_EXIT=1\nFAKE_RUNNER_HOLD=1\n";
const HELD: &str = "FAKE_RUNNER_HOLD=1\n";

fn proven(world: &World, job: u64) {
    world.wait_for_event("the proof of a job", 60, |events| {
        events.iter().any(|event| {
            event.label == "runner.delete"
                && event.field("name") == runner_name(job)
                && event.field("status") == "204"
        })
    });
}

fn held(world: &World, count: usize) -> u64 {
    world.wait_for_event("a held job", 60, |events| {
        events.iter().filter(|event| event.label == "jit").count() >= count
    });
    let job = reservation(&world.gh("jit")[count - 1]);
    wait_for("its stand-in", 20, || {
        world.pid_of(&jit_name(job)).is_some()
    });
    job
}

fn queue(world: &World, number: u64) -> TestResult<()> {
    world.queue_message(
        100 + number,
        &[push_offer(number, 10_000 + number, "main", WORKFLOW)],
    )
}

fn binary_mode(world: &World, mode: u32) -> TestResult<()> {
    std::fs::set_permissions(
        world.store.join("bin/velnor-runner"),
        std::fs::Permissions::from_mode(mode),
    )?;
    Ok(())
}

pub(super) fn run(test: &str) -> TestResult<()> {
    let mut world = World::new("T10", 1, true)?;
    world.configure_orb(FAILING)?;
    queue(&world, 1)?;
    world.spawn_controller(test, &[])?;
    proven(&world, failed(&world, 1));
    queue(&world, 2)?;
    proven(&world, failed(&world, 2));
    assert_eq!(controller_readiness(&world.state), Readiness::Ready);
    world.configure_orb(FAILING_HELD)?;
    queue(&world, 3)?;
    let third = held(&world, 3);
    binary_mode(&world, 0o775)?;
    world.release(&jit_name(third))?;
    wait_for("the machine_unavailable heartbeat", 30, || {
        world.heartbeat_phase().as_deref() == Some("machine_unavailable")
    });
    assert_eq!(controller_readiness(&world.state), Readiness::Degraded);
    closed_until_verified(&world)?;
    assert_eq!(world.finish(30)?, "Ok(Shutdown)");
    assert_clean(&world, "1");
    Ok(())
}

fn failed(world: &World, count: usize) -> u64 {
    world.wait_for_event("a failed job", 60, |events| {
        events.iter().filter(|event| event.label == "jit").count() >= count
    });
    reservation(&world.gh("jit")[count - 1])
}

fn closed_until_verified(world: &World) -> TestResult<()> {
    world.configure_orb(HELD)?;
    queue(world, 4)?;
    let marker = world.events().last().map_or(0, |event| event.seq);
    world.wait_for_event_unless(
        "two polls of the message of offer 4",
        60,
        |_| {
            world
                .gh("poll")
                .iter()
                .filter(|poll| poll.seq > marker && poll.field("returned") == "104")
                .count()
                >= 2
        },
        |_| {
            (acquire_ids(world) != ["1", "2", "3"])
                .then(|| "offer 4 was acquired after three failed runner exits".to_owned())
        },
    );
    binary_mode(world, 0o755)?;
    let fourth = held(world, 4);
    assert_eq!(world.heartbeat_phase().as_deref(), Some("runner_failing"));
    assert_eq!(controller_readiness(&world.state), Readiness::Degraded);
    world.release(&jit_name(fourth))?;
    wait_for("the ready heartbeat after a completed job", 30, || {
        world.heartbeat_phase().as_deref() == Some("ready")
    });
    assert_eq!(controller_readiness(&world.state), Readiness::Ready);
    world.stop()?;
    Ok(())
}
