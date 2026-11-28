// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::time::Duration;

use velnor_runner_host::{ConnectionId, wait_for_rust_runner_connection_drain};

use super::checks::{acquire_ids, jit_name, reservation, runner_name};
use super::child::SUPERVISED;
use super::events::wait_for;
use super::fixtures::{KEYCHAIN_REFERENCE, Spec, TestResult, WORKFLOW, host_toml_of, push_offer};
use super::harness::World;
use super::world_files::TOKEN_A;

fn polls_of(world: &World, repo: &str, id: &str, after: u64) -> usize {
    world
        .gh("poll")
        .iter()
        .filter(|poll| {
            poll.seq > after && poll.field("repo") == repo && poll.field("returned") == id
        })
        .count()
}

fn proven(world: &World, job: u64) {
    world.wait_for_event("the proof of a job", 40, |events| {
        events.iter().any(|event| {
            event.label == "runner.delete"
                && event.field("name").ends_with(&format!("-{job}"))
                && event.field("status") == "204"
        })
    });
}

fn drain_lab(world: &World, first: u64) -> TestResult<()> {
    world.write_marker("connection-drain/lab", TOKEN_A)?;
    wait_for("the census to record the draining connection", 30, || {
        world.census_tally("lab").is_some_and(|tally| {
            tally["slots"] == 1 && tally["drain"] == TOKEN_A && tally["admitting"] == false
        })
    });

    world.queue_message_for(
        "velnor-second",
        22,
        &[push_offer(2, 2201, "main", WORKFLOW)],
    )?;
    world.wait_for_event("the second connection to acquire", 60, |events| {
        events
            .iter()
            .any(|event| event.label == "acquire" && event.field("set") == "8")
    });

    world.queue_message(23, &[push_offer(3, 2301, "main", WORKFLOW)])?;
    let marker = world.events().last().map_or(0, |event| event.seq);
    world.wait_for_event_unless(
        "two polls of the message of the draining connection",
        40,
        |_| polls_of(world, "velnor-lab", "23", marker) >= 2,
        |_| {
            acquire_ids(world)
                .iter()
                .any(|ids| ids == "3")
                .then(|| "the draining connection acquired a job".to_owned())
        },
    );
    let id = ConnectionId::parse("lab")?;
    assert!(!wait_for_rust_runner_connection_drain(
        &world.state,
        &id,
        Duration::from_secs(2)
    )?);

    world.release(&jit_name(first))?;
    proven(world, first);
    assert!(wait_for_rust_runner_connection_drain(
        &world.state,
        &id,
        Duration::from_secs(20)
    )?);
    Ok(())
}

pub(super) fn run(test: &str) -> TestResult<()> {
    let (lab, second) = (Spec::lab(KEYCHAIN_REFERENCE), Spec::second());
    let mut world = World::with_extra("X2", 3, false, &[second])?;
    let both = host_toml_of(&world.root, 3, &[lab, second])?;
    let only_second = host_toml_of(&world.root, 3, &[second])?;
    world.write_host_toml_text(&both)?;
    world.configure_orb("FAKE_RUNNER_HOLD=1\n")?;
    world.queue_message(21, &[push_offer(1, 2101, "main", WORKFLOW)])?;
    world.spawn_controller(test, &[(SUPERVISED, "1")])?;
    let first = world.held_job(1);

    drain_lab(&world, first)?;
    let other = reservation(&world.gh("jit")[1]);

    assert_eq!(
        world.detach_within(test, "lab", &both, 1_500)?,
        "Ok(TimedOut)"
    );
    assert!(world.gh("scaleset.delete").is_empty());

    world.write_host_toml_text(&only_second)?;
    world.wait_census(
        "the census of the config without lab",
        &only_second,
        &["second"],
    );
    assert_eq!(
        world.census_tally("second").ok_or("tally")?["slots"],
        1,
        "the job of the other connection is still held"
    );
    assert!(world.pid_of(&jit_name(other)).is_some());
    world.plant_runner(&runner_name(9999), 7)?;
    world.hide_from_listing()?;
    assert_eq!(world.detach(test, "lab", &both)?, "Ok(Deleted)");
    let deletes = world.gh("scaleset.delete");
    assert_eq!(deletes.len(), 1);
    assert_eq!(deletes[0].field("repo"), "velnor-lab");
    assert_eq!(deletes[0].field("status"), "204");
    assert!(world.gh("runner.delete").iter().any(|event| {
        event.field("name") == runner_name(9999) && event.field("status") == "204"
    }));
    let state = world.fake_state()?;
    assert!(state["scale_sets"]["velnor-lab"].is_null());
    assert_eq!(state["scale_sets"]["velnor-second"], 8);
    assert!(!world.state.join("rust-runner-scale-sets/lab").exists());
    assert!(world.state.join("rust-runner-scale-sets/second").exists());

    world.queue_message_for(
        "velnor-second",
        24,
        &[push_offer(4, 2401, "main", WORKFLOW)],
    )?;
    world.wait_for_event("the other connection to keep serving", 60, |_| {
        acquire_ids(&world).iter().any(|ids| ids == "4")
    });
    assert!(world.pid_of(&jit_name(other)).is_some());
    world.release(&jit_name(other))?;
    proven(&world, other);
    world.release(&jit_name(reservation(&world.gh("jit")[2])))?;
    proven(&world, reservation(&world.gh("jit")[2]));

    world.stop()?;
    assert_eq!(world.finish(20)?, "Ok(Stopped)");
    Ok(())
}
