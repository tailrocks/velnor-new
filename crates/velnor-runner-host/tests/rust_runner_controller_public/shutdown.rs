// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::time::Instant;

use serde_json::json;

use super::checks::{assert_clean, deleted_messages, jit_name, reservation, runner_name};
use super::events::{process_exists, wait_for};
use super::fixtures::{TestResult, WORKFLOW, push_offer};
use super::harness::World;

pub(super) fn run(test: &str) -> TestResult<()> {
    super::credentials::run(test)?;
    let mut world = World::new("T5", 1, true)?;
    world.configure_orb("FAKE_RUNNER_HOLD=1\n")?;
    world.script_refusals(&json!({"velnor-mac-1": {"statuses": [409], "repeat_last": false}}))?;
    world.queue_message(51, &[push_offer(1, 5001, "main", WORKFLOW)])?;
    world.queue_message(52, &[push_offer(2, 5002, "main", WORKFLOW)])?;
    world.spawn_controller(test, &[])?;
    world.wait_for_event("the jit of job 1", 40, |events| {
        events.iter().any(|event| event.label == "jit")
    });
    let job = reservation(&world.gh("jit")[0]);
    let name = jit_name(job);
    wait_for("the stand-in and its child", 20, || {
        world.pid_of(&name).is_some() && world.seen.join(format!("{name}.child")).exists()
    });
    let pid = world.pid_of(&name).ok_or("no pid")?;
    let child = std::fs::read_to_string(world.seen.join(format!("{name}.child")))?
        .trim()
        .to_owned();
    world.wait_for_event("the deferred message of offer 2", 30, |events| {
        events
            .iter()
            .any(|event| event.label == "poll" && event.field("returned") == "52")
    });
    let before = world.events().last().map_or(0, |event| event.seq);
    let stopped = Instant::now();
    world.stop()?;
    assert_eq!(world.finish(20)?, "Ok(Shutdown)");
    assert!(stopped.elapsed().as_secs() <= 20);
    wait_for("the stand-in and its child to end", 10, || {
        !process_exists(&pid) && !process_exists(&child)
    });
    assert!(!world.store.join("jit").join(&name).exists());
    assert!(!world.store.join("work").join(job.to_string()).exists());
    let state: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(world.github.join("state.json"))?)?;
    assert_eq!(state["runners"].as_array().map(Vec::len), Some(0));
    let events = world.events();
    let session = events
        .iter()
        .rfind(|event| event.label == "session.delete")
        .ok_or("no session delete")?;
    let ps = events
        .iter()
        .find(|event| event.seq > before && event.is_orb("ps -eo"))
        .ok_or("no ps after the stop")?;
    let removals: Vec<&str> = events
        .iter()
        .filter(|event| event.label == "runner.delete" && event.field("name") == runner_name(job))
        .map(|event| event.field("status"))
        .collect();
    assert_eq!(
        removals,
        ["409", "204"],
        "the refused removal must be retried until it succeeds"
    );
    let removal = events
        .iter()
        .rfind(|event| event.label == "runner.delete" && event.field("name") == runner_name(job))
        .ok_or("no runner removal")?;
    assert!(ps.seq < session.seq && removal.seq < session.seq);
    assert_eq!(events.last().map(|event| event.seq), Some(session.seq));
    assert!(!deleted_messages(&world).contains(&"52".to_owned()));
    assert!(
        events
            .iter()
            .all(|event| event.label != "acquire" || event.seq < before)
    );
    assert_clean(&world, "1");
    drop(world);
    launching(test)
}

fn launching(test: &str) -> TestResult<()> {
    let mut world = World::new("T5b", 1, true)?;
    std::fs::write(world.seen.join("hold-acquire"), "")?;
    world.queue_message(53, &[push_offer(1, 5301, "main", WORKFLOW)])?;
    world.spawn_controller(test, &[])?;
    world.wait_for_event("the held acquire", 40, |events| {
        events.iter().any(|event| event.label == "acquire")
    });
    let held = world.events().last().map_or(0, |event| event.seq);
    world.stop()?;
    world.wait_for_event("the proof of the launching slot", 60, |events| {
        events.iter().any(|event| {
            event.seq > held && event.label == "runner.get" && event.field("name") == runner_name(1)
        })
    });
    std::fs::write(world.seen.join("release-acquire"), "")?;
    assert_eq!(world.finish(30)?, "Ok(Shutdown)");
    Ok(())
}
