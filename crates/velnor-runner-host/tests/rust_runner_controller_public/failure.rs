// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::os::unix::fs::PermissionsExt;

use velnor_runner_host::{Readiness, controller_readiness};

use super::checks::{acquire_ids, assert_clean, deleted_messages, runner_name, snapshot};
use super::events::{Event, wait_for};
use super::fixtures::{TestResult, WORKFLOW, push_offer, with_jit_failures, with_jit_identity};
use super::harness::World;

fn removed(world: &World, reservation: u64) -> bool {
    world.events().iter().any(|event| {
        event.label == "runner.delete"
            && event.field("name") == runner_name(reservation)
            && event.field("status") == "204"
    })
}

fn cleaned(world: &World, reservation: u64) -> bool {
    let line = format!("rust_runner_job=cleaned reservation={reservation} ");
    world
        .child_stderr()
        .lines()
        .any(|candidate| candidate.starts_with(&line))
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
        &[with_jit_failures(push_offer(1, 8001, "main", WORKFLOW), 3)],
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
    wait_for("the release of the slot of offer A", 40, || {
        cleaned(world, 1)
    });
    let attempts = world.gh("jit");
    assert_eq!(
        attempts.len(),
        3,
        "the JIT request of the acquired job was not retried to its budget"
    );
    assert!(
        attempts
            .iter()
            .all(|attempt| attempt.field("status") == "500")
    );
    world.queue_message(82, &[push_offer(2, 8002, "main", WORKFLOW)])?;
    world.wait_for_event("offer B", 40, |events| {
        events
            .iter()
            .any(|event| event.label == "acquire" && event.field("ids") == "2")
    });
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
            .all(|event| !event.text.contains("--reservation 1 ")),
        "a runner was started for offer A"
    );
    wait_for(
        "the release of the slot of offer B and the acknowledgement of message 81",
        40,
        || cleaned(world, 2) && deleted_messages(world).contains(&"81".to_owned()),
    );
    assert_eq!(
        acquire_ids(world),
        ["1", "2"],
        "offer A was acquired again in the same run"
    );
    assert_eq!(runner_starts(world), 1);
    Ok(())
}

fn probe_in_flight(world: &World) -> bool {
    std::fs::read_dir(&world.seen).is_ok_and(|entries| {
        entries
            .filter_map(Result::ok)
            .any(|entry| entry.file_name().to_string_lossy().starts_with("hang."))
    })
}

fn unsafe_binary(world: &World) -> TestResult<()> {
    world.configure_orb("FAKE_ORB_HANG=list\n")?;
    wait_for("a machine probe in flight", 40, || probe_in_flight(world));
    chmod_runner(world, 0o775)?;
    world.queue_message(83, &[push_offer(3, 8003, "main", WORKFLOW)])?;
    wait_for("the release of the slot of offer C", 40, || {
        cleaned(world, 3)
    });
    assert_eq!(runner_starts(world), 1, "a runner was started for offer C");
    std::fs::write(world.seen.join("unhang"), "")?;
    world.configure_orb("")?;
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
            (acquire_ids(world) != ["1", "2", "3"])
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
            >= 2
    });
    assert_eq!(acquire_ids(world), ["1", "2", "3", "4"]);
    Ok(())
}

pub(super) fn retries_a_failed_jit_for_the_acquired_job(test: &str) -> TestResult<()> {
    let mut world = World::new("T8b", 1, true)?;
    world.queue_message(
        81,
        &[with_jit_failures(push_offer(1, 8001, "main", WORKFLOW), 1)],
    )?;
    world.spawn_controller(test, &[])?;
    wait_for("the runner of offer A", 40, || runner_starts(&world) == 1);
    let statuses: Vec<String> = world
        .gh("jit")
        .iter()
        .map(|attempt| attempt.field("status").to_owned())
        .collect();
    assert_eq!(statuses, ["500", "200"]);
    assert_eq!(
        acquire_ids(&world),
        ["1"],
        "the offer was acquired again to retry its JIT request"
    );
    wait_for("the proof of offer A", 40, || removed(&world, 1));
    world.stop()?;
    assert_eq!(world.finish(30)?, "Ok(Shutdown)");
    assert_clean(&world, "1");
    Ok(())
}

pub(super) fn refuses_a_jit_for_another_runner(test: &str) -> TestResult<()> {
    let mut world = World::new("T8c", 1, true)?;
    for (message, request, field) in [(81, 1, "id"), (82, 2, "scale_set"), (83, 3, "name")] {
        world.queue_message(
            message,
            &[with_jit_identity(
                push_offer(request, 8000 + request, "main", WORKFLOW),
                field,
            )],
        )?;
    }
    world.spawn_controller(test, &[])?;
    wait_for("the proofs of the three refused runners", 60, || {
        (1..=3).all(|reservation| removed(&world, reservation))
    });
    assert_eq!(
        runner_starts(&world),
        0,
        "a runner started from a JIT configuration of another runner"
    );
    let mut acquired = acquire_ids(&world);
    acquired.sort();
    assert_eq!(acquired, ["1", "2", "3"]);
    assert_eq!(world.gh("jit").len(), 3, "a refused identity was retried");
    world.stop()?;
    assert_eq!(world.finish(30)?, "Ok(Shutdown)");
    assert_clean(&world, "1");
    Ok(())
}
