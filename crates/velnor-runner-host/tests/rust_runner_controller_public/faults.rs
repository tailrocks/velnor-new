// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde_json::{Value, json};
use velnor_runner_host::{Readiness, controller_readiness};

use super::checks::{jit_name, reservation};
use super::events::{process_exists, wait_for};
use super::fixtures::{TestResult, WORKFLOW, push_offer};
use super::harness::World;

const HELD: &str = "FAKE_RUNNER_HOLD=1\n";

fn registered_runners(world: &World) -> TestResult<usize> {
    let state: Value =
        serde_json::from_str(&std::fs::read_to_string(world.github.join("state.json"))?)?;
    Ok(state["runners"].as_array().map_or(0, Vec::len))
}

fn held_job(world: &mut World, test: &str) -> TestResult<(String, String)> {
    world.configure_orb(HELD)?;
    world.queue_message(141, &[push_offer(1, 14_001, "main", WORKFLOW)])?;
    world.spawn_controller(test, &[])?;
    world.wait_for_event("the jit of the job", 60, |events| {
        events.iter().any(|event| event.label == "jit")
    });
    let name = jit_name(reservation(&world.gh("jit")[0]));
    wait_for("its stand-in and child", 20, || {
        world.pid_of(&name).is_some() && world.seen.join(format!("{name}.child")).exists()
    });
    let pid = world.pid_of(&name).ok_or("no pid")?;
    let child = std::fs::read_to_string(world.seen.join(format!("{name}.child")))?
        .trim()
        .to_owned();
    Ok((pid, child))
}

pub(super) fn degraded_session(test: &str) -> TestResult<()> {
    let mut world = World::new("T14", 1, true)?;
    world.script_refusals(&json!({
        "poll.expired": {"statuses": [401], "repeat_last": true, "until": "seen/release-expired"},
        "session.refresh": {"statuses": [500], "repeat_last": true, "until": "seen/release-expired"}
    }))?;
    world.spawn_controller(test, &[])?;
    wait_for("the session_failing heartbeat", 90, || {
        world.heartbeat_phase().as_deref() == Some("session_failing")
    });
    assert_eq!(controller_readiness(&world.state), Readiness::Degraded);
    assert!(world.gh("session.refresh").len() >= 3);
    std::fs::write(world.seen.join("release-expired"), "")?;
    wait_for("the ready heartbeat after a good poll", 60, || {
        world.heartbeat_phase().as_deref() == Some("ready")
    });
    assert_eq!(controller_readiness(&world.state), Readiness::Ready);
    world.stop()?;
    assert_eq!(world.finish(30)?, "Ok(Shutdown)");
    Ok(())
}

pub(super) fn cleans_up_after_an_error(test: &str) -> TestResult<()> {
    let mut world = World::new("T15", 1, true)?;
    let (pid, child) = held_job(&mut world, test)?;
    world.script_refusals(&json!({
        "poll.expired": {"statuses": [401], "repeat_last": false},
        "session.refreshed": {"replies": [{"patch": {"messageQueueUrl": "http://queue.invalid/messages"}}]}
    }))?;
    let exit = world.finish(60)?;
    assert!(exit.starts_with("Err("), "{exit}");
    wait_for("the runner process to end", 10, || {
        !process_exists(&pid) && !process_exists(&child)
    });
    assert_eq!(registered_runners(&world)?, 0, "a runner stayed registered");
    assert!(
        !world.gh("session.delete").is_empty(),
        "the session was never closed"
    );
    assert_eq!(std::fs::read_dir(world.work())?.count(), 0);
    assert_eq!(std::fs::read_dir(world.stage())?.count(), 0);
    Ok(())
}

pub(super) fn leaves_everything_after_a_lost_lock(test: &str) -> TestResult<()> {
    let mut world = World::new("T16", 1, true)?;
    held_job(&mut world, test)?;
    std::fs::remove_file(world.state.join("daemon.lock"))?;
    let exit = world.finish(30)?;
    assert_eq!(exit, "Err(daemon lock held)", "{exit}");
    assert!(
        world.gh("session.delete").is_empty() && world.gh("runner.delete").is_empty(),
        "a controller that lost its daemon lock ran cleanup"
    );
    assert_eq!(registered_runners(&world)?, 1);
    Ok(())
}
