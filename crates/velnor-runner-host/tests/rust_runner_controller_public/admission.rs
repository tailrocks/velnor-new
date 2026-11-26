// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_runner_host::{Readiness, controller_readiness};

use super::checks::{
    acquire_ids, assert_clean, deleted_messages, jit_name, reservation, runner_name,
};
use super::events::wait_for;
use super::fixtures::{TestResult, WORKFLOW, push_offer};
use super::harness::World;

fn polls_returning(world: &World, id: &str, after: u64) -> usize {
    world
        .gh("poll")
        .iter()
        .filter(|poll| poll.seq > after && poll.field("returned") == id)
        .count()
}

pub(super) fn run(test: &str) -> TestResult<()> {
    let mut world = World::new("T6", 2, true)?;
    world.configure_orb("FAKE_RUNNER_HOLD=1\n")?;
    world.queue_message(61, &[push_offer(1, 6001, "main", WORKFLOW)])?;
    world.spawn_controller(test, &[])?;
    let first = held_job(&world, 1);
    drained(&world, first)?;
    let second = held_job(&world, 2);
    changed_config(&world, second)?;
    assert_eq!(world.finish(40)?, "Ok(ConfigChanged)");
    assert_clean(&world, "2");
    Ok(())
}

fn held_job(world: &World, count: usize) -> u64 {
    world.wait_for_event("a held job", 60, |events| {
        events.iter().filter(|event| event.label == "jit").count() >= count
    });
    let job = reservation(&world.gh("jit")[count - 1]);
    wait_for("its stand-in", 20, || {
        world.pid_of(&jit_name(job)).is_some()
    });
    job
}

fn drained(world: &World, first: u64) -> TestResult<()> {
    std::fs::write(world.state.join("drain"), "")?;
    world.queue_message(62, &[push_offer(2, 6002, "main", WORKFLOW)])?;
    world.release(&jit_name(first))?;
    world.wait_for_event("the proof of job 1", 40, |events| {
        events.iter().any(|event| {
            event.label == "runner.delete"
                && event.field("name") == runner_name(first)
                && event.field("status") == "204"
        })
    });
    let proof = world
        .gh("runner.delete")
        .iter()
        .map(|event| event.seq)
        .max()
        .unwrap_or_default();
    world.wait_for_event_unless(
        "two polls of the drained message",
        40,
        |_| polls_returning(world, "62", proof) >= 2,
        |_| {
            (acquire_ids(world) != ["1"])
                .then(|| "offer 2 was acquired while the host was draining".to_owned())
        },
    );
    assert_eq!(acquire_ids(world), ["1"]);
    assert!(!deleted_messages(world).contains(&"62".to_owned()));
    assert_eq!(controller_readiness(&world.state), Readiness::Draining);
    std::fs::remove_file(world.state.join("drain"))?;
    Ok(())
}

fn changed_config(world: &World, second: u64) -> TestResult<()> {
    world.write_host_toml(3)?;
    world.queue_message(63, &[push_offer(3, 6003, "main", WORKFLOW)])?;
    let marker = world.events().last().map_or(0, |event| event.seq);
    world.wait_for_event_unless(
        "two polls of the third message",
        40,
        |_| polls_returning(world, "63", marker) >= 2,
        |_| {
            (acquire_ids(world) != ["1", "2"])
                .then(|| "offer 3 was acquired after host.toml changed".to_owned())
        },
    );
    assert_eq!(acquire_ids(world), ["1", "2"]);
    assert!(!deleted_messages(world).contains(&"63".to_owned()));
    assert_eq!(controller_readiness(&world.state), Readiness::Reconciling);
    world.release(&jit_name(second))?;
    world.wait_for_event("the end of the controller", 60, |events| {
        events.iter().any(|event| event.label == "session.delete")
    });
    let events = world.events();
    let removal = events
        .iter()
        .find(|event| event.label == "runner.delete" && event.field("name") == runner_name(second))
        .ok_or("no removal of job 2")?;
    let session = events
        .iter()
        .find(|event| event.label == "session.delete")
        .ok_or("no session delete")?;
    assert!(removal.seq < session.seq);
    assert!(!deleted_messages(world).contains(&"63".to_owned()));
    Ok(())
}
