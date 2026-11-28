// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::time::{Duration, Instant};

use super::checks::{acquire_ids, assert_clean, jit_name, reservation, runner_name};
use super::events::wait_for;
use super::fixtures::{TestResult, WORKFLOW, push_offer};
use super::harness::World;

const TEST: &str =
    "wedged::rust_runner_controller_quarantines_a_slot_whose_job_dirs_cannot_be_removed";
const HOLD: &str = "FAKE_RUNNER_HOLD=1\n";
const WEDGED: &str = "FAKE_RUNNER_HOLD=1\nFAKE_GUEST_WEDGE=1\n";
const MOUNTED_TEST: &str =
    "wedged::rust_runner_controller_quarantines_a_slot_whose_job_dir_holds_a_mount";
const MOUNTED: &str = "FAKE_RUNNER_HOLD=1\nFAKE_GUEST_MOUNTS=/srv/velnor/gen/work/1/tools btrfs\n";

#[test]
fn rust_runner_controller_quarantines_a_slot_whose_job_dirs_cannot_be_removed() -> TestResult<()> {
    match super::child_root() {
        Some(root) => super::child::run(&root),
        None => run(TEST, WEDGED),
    }
}

#[test]
fn rust_runner_controller_quarantines_a_slot_whose_job_dir_holds_a_mount() -> TestResult<()> {
    match super::child_root() {
        Some(root) => super::child::run(&root),
        None => run(MOUNTED_TEST, MOUNTED),
    }
}

fn removals(world: &World) -> usize {
    world
        .events()
        .iter()
        .filter(|event| event.is_orb("exit 71"))
        .count()
}

fn run(test: &str, failing: &str) -> TestResult<()> {
    let mut world = World::new("T21", 1, true)?;
    world.configure_orb(HOLD)?;
    world.queue_message(
        61,
        &[
            push_offer(1, 6001, "main", WORKFLOW),
            push_offer(2, 6002, "main", WORKFLOW),
        ],
    )?;
    world.spawn_controller(test, &[])?;
    world.wait_for_event("the first jit", 40, |events| {
        events.iter().any(|event| event.label == "jit")
    });
    let first = reservation(&world.gh("jit")[0]);
    wait_for("the runner pid", 20, || {
        world.pid_of(&jit_name(first)).is_some()
    });
    let tools = world.work().join("1/tools");
    std::fs::create_dir_all(&tools)?;
    std::fs::write(tools.join("precious"), "kept")?;
    world.configure_orb(failing)?;
    world.release(&jit_name(first))?;
    wait_for("the one removal attempt", 40, || removals(&world) >= 1);
    let ticks = Instant::now() + Duration::from_secs(7);
    while Instant::now() < ticks {
        std::thread::sleep(Duration::from_millis(100));
    }
    assert_eq!(removals(&world), 1, "the cleanup was retried");
    assert_eq!(
        acquire_ids(&world),
        ["1"],
        "the quarantined slot was reused"
    );
    assert!(
        world
            .gh("runner.delete")
            .iter()
            .all(|event| event.field("name") != runner_name(first)),
        "the runner was removed although its files were left"
    );
    let stopped = Instant::now();
    world.stop()?;
    assert_eq!(world.finish(20)?, "Ok(Shutdown)");
    assert!(stopped.elapsed() < Duration::from_secs(20));
    assert!(
        world.child_stderr().contains("quarantined reservation"),
        "{}",
        world.child_stderr()
    );
    assert_clean(&world, "1");
    assert!(
        tools.join("precious").exists(),
        "a quarantined slot was emptied"
    );
    Ok(())
}

const STARTUP_TEST: &str =
    "wedged::rust_runner_controller_starts_with_a_wedged_job_dir_quarantined";

#[test]
fn rust_runner_controller_starts_with_a_wedged_job_dir_quarantined() -> TestResult<()> {
    match super::child_root() {
        Some(root) => super::child::run(&root),
        None => run_startup(),
    }
}

fn run_startup() -> TestResult<()> {
    let mut world = World::new("T22", 2, true)?;
    std::fs::create_dir_all(world.work().join("99/data"))?;
    world.configure_orb(WEDGED)?;
    world.queue_message(
        71,
        &[
            push_offer(1, 7001, "main", WORKFLOW),
            push_offer(2, 7002, "main", WORKFLOW),
        ],
    )?;
    world.spawn_controller(STARTUP_TEST, &[])?;
    world.wait_for_event("the first jit", 60, |events| {
        events.iter().any(|event| event.label == "jit")
    });
    let first = reservation(&world.gh("jit")[0]);
    assert_eq!(first, 1, "the lowest id that is not quarantined");
    assert!(
        world
            .child_stderr()
            .contains("quarantined reservation 99 at startup"),
        "{}",
        world.child_stderr()
    );
    assert!(world.work().join("99/data").exists());
    wait_for("its stand-in", 20, || {
        world.pid_of(&jit_name(first)).is_some()
    });
    let ticks = Instant::now() + Duration::from_secs(5);
    while Instant::now() < ticks {
        std::thread::sleep(Duration::from_millis(100));
    }
    assert_eq!(
        acquire_ids(&world),
        ["1"],
        "the quarantined slot was reused"
    );
    world.stop()?;
    assert_eq!(world.finish(20)?, "Ok(Shutdown)");
    Ok(())
}

const CONFIG_TEST: &str =
    "wedged::rust_runner_controller_follows_a_config_change_with_a_quarantined_slot";

#[test]
fn rust_runner_controller_follows_a_config_change_with_a_quarantined_slot() -> TestResult<()> {
    match super::child_root() {
        Some(root) => super::child::run(&root),
        None => run_config_change(),
    }
}

fn run_config_change() -> TestResult<()> {
    let mut world = World::new("T23", 2, true)?;
    std::fs::create_dir_all(world.work().join("99/data"))?;
    world.configure_orb("FAKE_GUEST_WEDGE=1\n")?;
    world.queue_message(81, &[push_offer(1, 8001, "main", WORKFLOW)])?;
    world.spawn_controller(CONFIG_TEST, &[])?;
    world.wait_for_event("the first jit", 60, |events| {
        events.iter().any(|event| event.label == "jit")
    });
    let first = reservation(&world.gh("jit")[0]);
    world.configure_orb("")?;
    world.wait_for_event("the proof of the job", 60, |events| {
        events.iter().any(|event| {
            event.label == "runner.delete" && event.field("name") == runner_name(first)
        })
    });
    world.write_host_toml(1)?;
    assert_eq!(
        world.finish(40)?,
        "Ok(ConfigChanged)",
        "a quarantined slot held the daemon on a stale host.toml"
    );
    assert!(world.work().join("99/data").exists());
    Ok(())
}
