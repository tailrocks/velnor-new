// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::os::unix::fs::PermissionsExt;

use super::checks::{assert_clean, runner_name};
use super::child::ORB_DEADLINE_MS;
use super::events::{process_exists, wait_for};
use super::fixtures::{TestResult, WORKFLOW, push_offer};
use super::harness::World;

fn list_calls(world: &World) -> usize {
    world
        .events()
        .iter()
        .filter(|event| event.is_orb("list -f json"))
        .count()
}

fn hung_pids(world: &World) -> Vec<String> {
    std::fs::read_dir(&world.seen)
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .filter(|entry| entry.file_name().to_string_lossy().starts_with("hang."))
        .filter_map(|entry| std::fs::read_to_string(entry.path()).ok())
        .map(|pid| pid.trim().to_owned())
        .collect()
}

pub(super) fn run(test: &str) -> TestResult<()> {
    let mut world = World::new("T17", 1, true)?;
    world.spawn_controller(test, &[(ORB_DEADLINE_MS, "1500")])?;
    world.wait_for_event("the first poll", 60, |events| {
        events.iter().any(|event| event.label == "poll")
    });
    world.configure_orb("FAKE_ORB_HANG=list\n")?;
    std::fs::set_permissions(
        world.store.join("bin/velnor-runner"),
        std::fs::Permissions::from_mode(0o775),
    )?;
    world.queue_message(171, &[push_offer(1, 17_001, "main", WORKFLOW)])?;
    wait_for(
        "two hung machine checks, each ended by its deadline",
        90,
        || list_calls(&world) >= 2,
    );
    wait_for("the machine_unavailable heartbeat", 20, || {
        world.heartbeat_phase().as_deref() == Some("machine_unavailable")
    });
    wait_for("the first hung call to be killed", 20, || {
        let pids = hung_pids(&world);
        !pids.is_empty() && !process_exists(&pids[0])
    });
    world.configure_orb("")?;
    std::fs::set_permissions(
        world.store.join("bin/velnor-runner"),
        std::fs::Permissions::from_mode(0o755),
    )?;
    wait_for("the ready heartbeat after the machine answered", 60, || {
        world.heartbeat_phase().as_deref() == Some("ready")
    });
    world.wait_for_event("the proof of the failed launch", 60, |events| {
        events
            .iter()
            .any(|event| event.label == "runner.delete" && event.field("name") == runner_name(1))
    });
    world.stop()?;
    assert_eq!(world.finish(30)?, "Ok(Shutdown)");
    assert_clean(&world, "1");
    Ok(())
}
