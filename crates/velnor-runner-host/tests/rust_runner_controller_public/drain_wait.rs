// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::time::{Duration, Instant};

use velnor_runner_host::wait_for_rust_runner_drain;

use super::checks::{jit_name, reservation, runner_name};
use super::child::SUPERVISED;
use super::events::wait_for;
use super::fixtures::{TestResult, WORKFLOW, push_offer};
use super::harness::World;
use super::world_files::{CENSUS, TOKEN_A, TOKEN_B};

fn held_slots(world: &World) -> Option<(u64, String)> {
    let tally = world.census_tally("lab")?;
    let drain = world.census()?["drain"].as_str()?.to_owned();
    Some((tally["slots"].as_u64()?, drain))
}

fn stale_markers(world: &World) -> TestResult<()> {
    world.write_marker("drain", TOKEN_A)?;
    assert!(!wait_for_rust_runner_drain(
        &world.state,
        Duration::from_secs(1)
    )?);
    world.write_marker("drain", TOKEN_B)?;
    assert!(wait_for_rust_runner_drain(
        &world.state,
        Duration::from_secs(2)
    )?);
    world.age(CENSUS, -10_000)?;
    assert!(!wait_for_rust_runner_drain(
        &world.state,
        Duration::from_secs(1)
    )?);
    world.age(CENSUS, 10_000)?;
    assert!(wait_for_rust_runner_drain(
        &world.state,
        Duration::from_secs(2)
    )?);
    Ok(())
}

pub(super) fn run(test: &str) -> TestResult<()> {
    let mut world = World::new("D1", 1, true)?;
    world.configure_orb("FAKE_RUNNER_HOLD=1\n")?;
    world.queue_message(11, &[push_offer(1, 1101, "main", WORKFLOW)])?;
    world.spawn_controller(test, &[(SUPERVISED, "1")])?;
    world.wait_for_event("a held job", 60, |events| {
        events.iter().any(|event| event.label == "jit")
    });
    let job = reservation(&world.gh("jit")[0]);
    wait_for("its stand-in", 20, || {
        world.pid_of(&jit_name(job)).is_some()
    });

    world.write_marker("drain", TOKEN_A)?;
    wait_for(
        "the census to record the marker and the held slot",
        30,
        || held_slots(&world) == Some((1, TOKEN_A.to_owned())),
    );
    assert!(!wait_for_rust_runner_drain(
        &world.state,
        Duration::from_secs(2)
    )?);

    let marker = world.state.join("drain");
    let replacer = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(300));
        std::fs::write(marker, TOKEN_B)
    });
    let started = Instant::now();
    assert!(!wait_for_rust_runner_drain(
        &world.state,
        Duration::from_secs(15)
    )?);
    assert!(started.elapsed() < Duration::from_secs(5));
    replacer.join().map_err(|_| "marker replacer")??;

    world.release(&jit_name(job))?;
    world.wait_for_event("the proof of the job", 40, |events| {
        events.iter().any(|event| {
            event.label == "runner.delete"
                && event.field("name") == runner_name(job)
                && event.field("status") == "204"
        })
    });
    assert!(wait_for_rust_runner_drain(
        &world.state,
        Duration::from_secs(20)
    )?);

    world.stop()?;
    assert_eq!(world.finish(20)?, "Ok(Stopped)");
    assert!(wait_for_rust_runner_drain(
        &world.state,
        Duration::from_secs(2)
    )?);
    stale_markers(&world)?;
    world.age(CENSUS, 31_000)?;
    assert!(!wait_for_rust_runner_drain(
        &world.state,
        Duration::from_secs(1)
    )?);
    std::fs::remove_file(world.state.join(CENSUS))?;
    assert!(!wait_for_rust_runner_drain(
        &world.state,
        Duration::from_secs(1)
    )?);
    Ok(())
}
