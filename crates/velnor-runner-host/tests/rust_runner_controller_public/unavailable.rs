// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde_json::{Value, json};

use super::checks::{acquire_ids, assert_clean, deleted_messages, runner_name};
use super::fixtures::{TestResult, WORKFLOW, push_offer};
use super::harness::World;

pub(super) fn run(test: &str) -> TestResult<()> {
    deferred(
        test,
        "T9",
        &json!({"repo.get": {"after_session": true, "replies": [{}, {"status": 503}]}}),
    )?;
    deferred(
        test,
        "T9b",
        &json!({"run.get": {"after_session": true, "replies": [{}, {}, {}, {"status": 503}]}}),
    )
}

pub(super) fn rate_limited(test: &str) -> TestResult<()> {
    for (tag, reply) in [
        ("T9c", json!({"status": 429})),
        (
            "T9d",
            json!({"status": 403, "headers": {"retry-after": "1"}}),
        ),
        (
            "T9e",
            json!({"status": 403, "headers": {"x-ratelimit-remaining": "0"}}),
        ),
    ] {
        deferred(
            test,
            tag,
            &json!({"run.get": {"after_session": true, "replies": [{}, {}, {}, reply]}}),
        )?;
    }
    Ok(())
}

pub(super) fn refused_for_good(test: &str) -> TestResult<()> {
    let mut world = World::new("T9f", 1, true)?;
    world.script_refusals(
        &json!({"run.get": {"after_session": true, "replies": [{}, {}, {}, {"status": 403}]}}),
    )?;
    world.queue_message(91, &[push_offer(1, 9001, "main", WORKFLOW)])?;
    world.spawn_controller(test, &[])?;
    world.wait_for_event_unless(
        "message 91 deleted after a 403 without rate-limit signals",
        60,
        |_| deleted_messages(&world).contains(&"91".to_owned()),
        |_| {
            world
                .child_stderr()
                .lines()
                .any(|line| line == "source_admission=deferred reason=source_api_unavailable")
                .then(|| "a 403 without rate-limit signals was deferred as retryable".to_owned())
        },
    );
    assert!(
        acquire_ids(&world).is_empty(),
        "a refused source check must not acquire"
    );
    world.stop()?;
    assert_eq!(world.finish(20)?, "Ok(Shutdown)");
    Ok(())
}

fn deferred(test: &str, tag: &'static str, script: &Value) -> TestResult<()> {
    let mut world = World::new(tag, 1, true)?;
    world.script_refusals(script)?;
    world.queue_message(91, &[push_offer(1, 9001, "main", WORKFLOW)])?;
    world.spawn_controller(test, &[])?;
    world.wait_for_event_unless(
        "offer 1 after its check was answered",
        60,
        |_| acquire_ids(&world) == ["1"],
        |_| {
            deleted_messages(&world)
                .contains(&"91".to_owned())
                .then(|| "message 91 was deleted although its check was never answered".to_owned())
        },
    );
    world.wait_for_event("the proof of the job", 40, |events| {
        events.iter().any(|event| {
            event.label == "runner.delete"
                && event.field("name") == runner_name(1)
                && event.field("status") == "204"
        })
    });
    assert!(
        world
            .child_stderr()
            .lines()
            .any(|line| line == "source_admission=deferred reason=source_api_unavailable")
    );
    let opened = world.gh("session.create")[0].seq;
    let polls = world
        .gh("poll")
        .iter()
        .filter(|poll| poll.seq > opened && poll.field("returned") == "91")
        .count();
    assert!(polls >= 2, "message 91 was polled {polls} times");
    world.stop()?;
    assert_eq!(world.finish(20)?, "Ok(Shutdown)");
    assert_clean(&world, "1");
    Ok(())
}
