// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde_json::json;
use velnor_runner_host::{Readiness, controller_readiness};

use super::checks::{
    acquire_ids, assert_clean, call_number, deleted_messages, heartbeat_advances, runner_name,
    snapshot,
};
use super::events::{Event, wait_for};
use super::fixtures::{TestResult, WORKFLOW, push_offer};
use super::harness::World;

const HOLD_A: &str = "FAKE_RUNNER_PS_PID=7001\nFAKE_ORB_PS_IMMORTAL=7001\nFAKE_ORB_PS_FAIL=2\n";

fn acquire_with_ids(events: &[Event], id: &str) -> Option<Event> {
    events
        .iter()
        .find(|event| event.label == "acquire" && event.field("ids") == id)
        .cloned()
}

pub(super) fn run(test: &str) -> TestResult<()> {
    let id = std::process::Command::new("id").arg("-u").output()?;
    assert_ne!(
        String::from_utf8_lossy(&id.stdout).trim(),
        "0",
        "run this test as a non-root user: root removes a 0500 directory and job C would prove nothing"
    );
    let mut world = World::new("T3", 1, true)?;
    world.configure_orb(HOLD_A)?;
    world.script_refusals(&json!({
        "velnor-mac-2": {"statuses": [400, 409], "repeat_last": true, "until": "seen/release-runner"}
    }))?;
    queue_next(&world, 31)?;
    world.spawn_controller(test, &[])?;
    held_by_processes(&world)?;
    world.configure_orb("")?;
    std::fs::write(world.seen.join("release"), "")?;
    held_by_the_runner(&world)?;
    world.configure_orb("FAKE_RUNNER_LOCK=1\n")?;
    std::fs::write(world.seen.join("release-runner"), "")?;
    reclaimed_files(&world)?;
    world.wait_for_event(
        "the proof of job D: the 0500 work directory left by job C must not hold its slot",
        90,
        |events| {
            events.iter().any(|event| {
                event.label == "runner.delete"
                    && event.field("name") == runner_name(4)
                    && event.field("status") == "204"
            })
        },
    );
    let events = world.events();
    let acquire = acquire_with_ids(&events, "4").ok_or("no acquire for D")?;
    let state = snapshot(&world, &acquire);
    assert!(
        !state.runners.contains(&runner_name(3))
            && !state.work.contains(&"3".to_owned())
            && !state.jit.contains(&"3.jit".to_owned()),
        "{state:?}"
    );
    world.stop()?;
    assert_eq!(world.finish(20)?, "Ok(Shutdown)");
    assert_clean(&world, "1");
    assert_eq!(acquire_ids(&world), ["1", "2", "3", "4"]);
    assert_eq!(std::fs::read_dir(world.store.join("jit"))?.count(), 0);
    assert_eq!(std::fs::read_dir(world.store.join("work"))?.count(), 0);
    let state: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(world.github.join("state.json"))?)?;
    assert_eq!(state["runners"].as_array().map(Vec::len), Some(0));
    Ok(())
}

fn assert_held(world: &World, job: &str, allowed_deletes: &[&str], acquired: &[&str]) {
    assert_eq!(
        controller_readiness(&world.state),
        Readiness::Ready,
        "{job}"
    );
    let marker = world.events().last().map_or(0, |event| event.seq);
    world.wait_for_event_unless(
        "two polls while the slot is held",
        30,
        |events| {
            events
                .iter()
                .filter(|event| event.label == "poll" && event.seq > marker)
                .count()
                >= 2
        },
        |_| {
            (acquire_ids(world) != acquired).then(|| {
                format!(
                    "{job}: a new acquire during the hold: {:?}",
                    acquire_ids(world)
                )
            })
        },
    );
    for id in deleted_messages(world) {
        assert!(
            allowed_deletes.contains(&id.as_str()),
            "{job}: queued message {id} deleted"
        );
    }
    assert_eq!(acquire_ids(world), acquired, "{job}");
}

fn queue_next(world: &World, message: u64) -> TestResult<()> {
    let request = message - 30;
    world.queue_message(
        message,
        &[push_offer(request, 3000 + request, "main", WORKFLOW)],
    )
}

fn held_by_processes(world: &World) -> TestResult<()> {
    world.wait_for_event("job A", 40, |events| {
        events.iter().any(Event::is_runner_start)
    });
    queue_next(world, 32)?;
    let started = world
        .events()
        .into_iter()
        .find(Event::is_runner_start)
        .ok_or("no job A")?;
    heartbeat_advances(world, 4);
    world.wait_for_event(
        "four ps calls and a kill of 7001 after job A",
        40,
        |events| {
            let after: Vec<_> = events
                .iter()
                .filter(|event| event.seq > started.seq)
                .collect();
            after.iter().filter(|event| event.is_orb("ps -eo")).count() >= 4
                && after.iter().any(|event| event.is_orb("kill -9 7001"))
        },
    );
    assert_held(world, "job A", &["31"], &["1"]);
    Ok(())
}

fn held_by_the_runner(world: &World) -> TestResult<()> {
    world.wait_for_event("job B", 60, |events| {
        acquire_with_ids(events, "2").is_some()
    });
    queue_next(world, 33)?;
    let events = world.events();
    let acquire = acquire_with_ids(&events, "2").ok_or("no acquire for B")?;
    let state = snapshot(world, &acquire);
    assert!(
        state
            .ps
            .lines()
            .all(|line| !line.trim_start().starts_with("7001 ")),
        "{}",
        state.ps
    );
    assert!(
        !state.runners.contains(&runner_name(1)),
        "{:?}",
        state.runners
    );
    assert!(!state.jit.contains(&"1.jit".to_owned()) && !state.work.contains(&"1".to_owned()));
    let last_ps = events
        .iter()
        .rfind(|event| event.seq < acquire.seq && event.is_orb("ps -eo"))
        .ok_or("no ps before B")?;
    let call = call_number(last_ps);
    let status = std::fs::read_to_string(world.seen.join(format!("calls/{call}.status")))?;
    assert_eq!(status.trim(), "0");
    let output =
        std::fs::read_to_string(world.seen.join(format!("calls/{call}.out"))).unwrap_or_default();
    assert!(!output.contains("7001"), "{output}");
    world.wait_for_event("the runner of job B", 20, |events| {
        events
            .iter()
            .filter(|event| event.is_runner_start())
            .count()
            >= 2
    });
    let ended = world
        .events()
        .into_iter()
        .filter(Event::is_runner_start)
        .nth(1)
        .ok_or("no job B")?;
    heartbeat_advances(world, 4);
    world.wait_for_event("four refused removals of B", 40, |events| {
        events
            .iter()
            .filter(|event| {
                event.seq > ended.seq
                    && event.label == "runner.delete"
                    && event.field("name") == runner_name(2)
            })
            .count()
            >= 4
    });
    assert_held(world, "job B", &["31", "32"], &["1", "2"]);
    Ok(())
}

fn reclaimed_files(world: &World) -> TestResult<()> {
    world.wait_for_event("job C", 60, |events| {
        acquire_with_ids(events, "3").is_some()
    });
    queue_next(world, 34)?;
    let events = world.events();
    let removal = events
        .iter()
        .find(|event| {
            event.label == "runner.delete"
                && event.field("name") == runner_name(2)
                && event.field("status") == "204"
        })
        .ok_or("B was never removed")?;
    let acquire = acquire_with_ids(&events, "3").ok_or("no acquire for C")?;
    assert!(acquire.seq > removal.seq);
    let state = snapshot(world, &acquire);
    assert!(!state.runners.contains(&runner_name(2)) && !state.work.contains(&"2".to_owned()));
    let locked = world.seen.join("3.jit.locked");
    wait_for(
        "job C to leave a 0500 directory in its work dir",
        20,
        || locked.exists(),
    );
    world.wait_for_event(
        "job D to be acquired: the 0500 work directory left by job C must not hold its slot",
        90,
        |events| acquire_with_ids(events, "4").is_some(),
    );
    Ok(())
}
