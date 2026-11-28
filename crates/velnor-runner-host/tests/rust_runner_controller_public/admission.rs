// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::os::unix::fs::PermissionsExt;

use velnor_runner_host::{Readiness, controller_readiness};

use super::checks::{
    acquire_ids, assert_clean, deleted_messages, jit_name, reservation, runner_name,
};
use super::child::PASSED_MAX_JOBS;
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
    mismatched_config(&mut world, test)?;
    world.queue_message(61, &[push_offer(1, 6001, "main", WORKFLOW)])?;
    world.spawn_controller(test, &[])?;
    let first = held_job(&world, 1);
    drained(&world, first)?;
    let second = held_job(&world, 2);
    unreadable_config(&world)?;
    changed_config(&world, second)?;
    assert_eq!(world.finish()?, "Ok(ConfigChanged)");
    assert_clean(&world, "2");
    Ok(())
}

fn mismatched_config(world: &mut World, test: &str) -> TestResult<()> {
    world.spawn_controller(test, &[(PASSED_MAX_JOBS, "1")])?;
    assert_eq!(world.finish()?, "Ok(ConfigChanged)");
    assert!(
        world.gh("session.create").is_empty() && world.gh("poll").is_empty(),
        "a controller whose passed config differs from host.toml must exit before it opens a session or polls"
    );
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

fn unreadable_config(world: &World) -> TestResult<()> {
    let path = world.state.join("host.toml");
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o000))?;
    world.queue_message(63, &[push_offer(3, 6003, "main", WORKFLOW)])?;
    let marker = world.events().last().map_or(0, |event| event.seq);
    world.wait_for_event_unless(
        "two polls of the third message while host.toml is unreadable",
        40,
        |_| polls_returning(world, "63", marker) >= 2,
        |_| {
            (acquire_ids(world) != ["1", "2"])
                .then(|| "offer 3 was acquired while host.toml was unreadable".to_owned())
        },
    );
    assert!(!deleted_messages(world).contains(&"63".to_owned()));
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))?;
    let third = held_job(world, 3);
    world.release(&jit_name(third))?;
    world.wait_for_event("the proof of job 3", 40, |events| {
        events.iter().any(|event| {
            event.label == "runner.delete"
                && event.field("name") == runner_name(third)
                && event.field("status") == "204"
        })
    });
    Ok(())
}

fn changed_config(world: &World, second: u64) -> TestResult<()> {
    world.write_host_toml(3)?;
    world.queue_message(64, &[push_offer(4, 6004, "main", WORKFLOW)])?;
    let marker = world.events().last().map_or(0, |event| event.seq);
    world.wait_for_event_unless(
        "two polls of the fourth message",
        40,
        |_| polls_returning(world, "64", marker) >= 2,
        |_| {
            (acquire_ids(world) != ["1", "2", "3"])
                .then(|| "offer 4 was acquired after host.toml changed".to_owned())
        },
    );
    assert_eq!(acquire_ids(world), ["1", "2", "3"]);
    assert!(!deleted_messages(world).contains(&"64".to_owned()));
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
    assert!(!deleted_messages(world).contains(&"64".to_owned()));
    Ok(())
}

pub(super) fn probes_the_machine_while_idle(test: &str) -> TestResult<()> {
    let mut world = World::new("T6b", 1, true)?;
    world.spawn_controller(test, &[])?;
    wait_for("the ready heartbeat", 40, || {
        world.heartbeat_phase().as_deref() == Some("ready")
    });
    world.configure_orb(&format!(
        "FAKE_ORB_LIST={}\n",
        world.root.join("no-such-list.json").display()
    ))?;
    wait_for(
        "the machine_unavailable heartbeat of an idle host",
        40,
        || world.heartbeat_phase().as_deref() == Some("machine_unavailable"),
    );
    assert_eq!(controller_readiness(&world.state), Readiness::Degraded);
    world.queue_message(65, &[push_offer(1, 6501, "main", WORKFLOW)])?;
    let marker = world.events().last().map_or(0, |event| event.seq);
    world.wait_for_event_unless(
        "two polls of the message while the machine is down",
        40,
        |_| polls_returning(&world, "65", marker) >= 2,
        |_| {
            (!acquire_ids(&world).is_empty())
                .then(|| "an offer was acquired while the machine was unavailable".to_owned())
        },
    );
    world.configure_orb("")?;
    wait_for("the ready heartbeat after the machine returned", 40, || {
        world.heartbeat_phase().as_deref() == Some("ready")
    });
    world.wait_for_event("the offer after the machine returned", 60, |_| {
        acquire_ids(&world) == ["1"]
    });
    world.stop()?;
    assert_eq!(world.finish()?, "Ok(Shutdown)");
    Ok(())
}
