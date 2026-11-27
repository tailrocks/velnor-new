// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::os::unix::fs::PermissionsExt;

use velnor_runner_host::{Readiness, controller_readiness};

use super::checks::{acquire_ids, assert_clean, deleted_messages, runner_name, snapshot};
use super::events::{Event, wait_for};
use super::fixtures::{TestResult, WORKFLOW, push_offer, with_jit_failure};
use super::harness::World;

fn removed(world: &World, reservation: u64) -> bool {
    world.events().iter().any(|event| {
        event.label == "runner.delete"
            && event.field("name") == runner_name(reservation)
            && event.field("status") == "204"
    })
}

fn polls_of(world: &World, message: &str, after: u64) -> usize {
    world
        .gh("poll")
        .iter()
        .filter(|poll| poll.seq > after && poll.field("returned") == message)
        .count()
}

fn runner_starts(world: &World) -> usize {
    world
        .events()
        .iter()
        .filter(|event| event.is_runner_start())
        .count()
}

fn last_seq(world: &World) -> u64 {
    world.events().last().map_or(0, |event| event.seq)
}

fn chmod_runner(world: &World, mode: u32) -> TestResult<()> {
    std::fs::set_permissions(
        world.store.join("bin/velnor-runner"),
        std::fs::Permissions::from_mode(mode),
    )?;
    Ok(())
}

pub(super) fn run(test: &str) -> TestResult<()> {
    let mut world = World::new("T8", 1, true)?;
    world.queue_message(
        81,
        &[with_jit_failure(push_offer(1, 8001, "main", WORKFLOW))],
    )?;
    world.spawn_controller(test, &[])?;
    failed_jit(&world)?;
    unsafe_binary(&world)?;
    world.stop()?;
    assert_eq!(world.finish(30)?, "Ok(Shutdown)");
    assert_clean(&world, "1");
    Ok(())
}

fn failed_jit(world: &World) -> TestResult<()> {
    wait_for("the proof of the failed launch of offer A", 40, || {
        removed(world, 1)
    });
    let failed = world.gh("jit")[0].clone();
    assert_eq!(failed.field("status"), "500");
    world.queue_message(82, &[push_offer(2, 8002, "main", WORKFLOW)])?;
    world.wait_for_event("offer B", 40, |events| {
        events
            .iter()
            .any(|event| event.label == "acquire" && event.field("ids") == "2")
    });
    assert_eq!(acquire_ids(world), ["1", "2"]);
    let events = world.events();
    let removal = events
        .iter()
        .find(|event| event.label == "runner.delete" && event.field("name") == runner_name(1))
        .ok_or("no removal of the runner of offer A")?;
    let acquire = events
        .iter()
        .find(|event| event.label == "acquire" && event.field("ids") == "2")
        .ok_or("no acquire of offer B")?;
    assert!(acquire.seq > removal.seq);
    assert!(!snapshot(world, acquire).runners.contains(&runner_name(1)));
    assert!(
        events
            .iter()
            .filter(|event| event.is_runner_start())
            .all(|event| !event.text.contains("/jit/1.jit")),
        "a runner was started for offer A"
    );
    world.wait_for_event_unless(
        "the redelivery of the message of offer A",
        40,
        |events| {
            events.iter().any(|event| {
                event.label == "poll" && event.seq > failed.seq && event.field("returned") == "81"
            })
        },
        |_| {
            deleted_messages(world)
                .contains(&"81".to_owned())
                .then(|| "message 81 was deleted although offer A never ran".to_owned())
        },
    );
    world.wait_for_event_unless(
        "offer A again",
        40,
        |_| acquire_ids(world) == ["1", "2", "1"],
        |_| {
            deleted_messages(world).contains(&"81".to_owned()).then(|| {
                "message 81 was deleted although offer A was not acquired again".to_owned()
            })
        },
    );
    wait_for("the proof of the second launch of offer A", 40, || {
        removed(world, 3) && deleted_messages(world).contains(&"81".to_owned())
    });
    assert_eq!(runner_starts(world), 2);
    Ok(())
}

fn unsafe_binary(world: &World) -> TestResult<()> {
    chmod_runner(world, 0o775)?;
    world.queue_message(83, &[push_offer(3, 8003, "main", WORKFLOW)])?;
    wait_for("the proof of offer C", 40, || removed(world, 4));
    assert_eq!(runner_starts(world), 2, "a runner was started for offer C");
    wait_for("the machine_unavailable heartbeat", 20, || {
        world.heartbeat_phase().as_deref() == Some("machine_unavailable")
    });
    assert_eq!(controller_readiness(&world.state), Readiness::Degraded);
    world.queue_message(84, &[push_offer(4, 8004, "main", WORKFLOW)])?;
    let marker = last_seq(world);
    world.wait_for_event_unless(
        "two polls of the message of offer D",
        40,
        |_| polls_of(world, "84", marker) >= 2,
        |_| {
            (acquire_ids(world) != ["1", "2", "1", "3"])
                .then(|| "offer D was acquired while the binary was unsafe".to_owned())
        },
    );
    chmod_runner(world, 0o755)?;
    wait_for("the ready heartbeat", 20, || {
        world.heartbeat_phase().as_deref() == Some("ready")
    });
    assert_eq!(controller_readiness(&world.state), Readiness::Ready);
    world.wait_for_event("offer D after the binary was restored", 40, |events| {
        events
            .iter()
            .filter(|event| Event::is_runner_start(event))
            .count()
            >= 3
    });
    assert_eq!(acquire_ids(world), ["1", "2", "1", "3", "4"]);
    Ok(())
}
