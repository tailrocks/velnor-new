// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::{Value, json};

use super::checks::{acquire_ids, assert_clean, deleted_messages, runner_name};
use super::fixtures::{TestResult, WORKFLOW, push_offer};
use super::harness::World;

pub(super) fn run(test: &str) -> TestResult<()> {
    deferred(
        test,
        "T9",
        &json!({"repo.get": {"after_session": true, "replies": [{}, {"status": 503}]}}),
        |_| Ok(()),
    )?;
    deferred(
        test,
        "T9b",
        &json!({"run.get": {"after_session": true, "replies": [{}, {}, {}, {"status": 503}]}}),
        |_| Ok(()),
    )
}

pub(super) fn rate_limited(test: &str) -> TestResult<()> {
    let soon = (SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs() + 2).to_string();
    for (tag, reply) in [
        (
            "T9c",
            json!({"status": 429, "headers": {"retry-after": "1"}}),
        ),
        (
            "T9d",
            json!({"status": 403, "headers": {"retry-after": "1"}}),
        ),
        (
            "T9e",
            json!({"status": 403, "headers": {"x-ratelimit-remaining": "0", "x-ratelimit-reset": soon}}),
        ),
    ] {
        deferred(
            test,
            tag,
            &json!({"run.get": {"after_session": true, "replies": [{}, {}, {}, reply]}}),
            |_| Ok(()),
        )?;
    }
    Ok(())
}

pub(super) fn held_until_reset(test: &str) -> TestResult<()> {
    let reset = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs() + 30;
    let limited = json!({"status": 403, "headers": {"x-ratelimit-remaining": "0", "x-ratelimit-reset": reset.to_string()}});
    deferred(
        test,
        "T9g",
        &json!({"run.get": {"after_session": true, "replies": [{}, {}, {}, limited]}}),
        |world| {
            let reads = world.gh("run.get");
            let refused = reads
                .iter()
                .position(|read| read.field("status") == "403")
                .ok_or("no 403 answer to a run lookup")?;
            let next = reads
                .get(refused + 1)
                .ok_or("no run lookup after the 403")?;
            assert!(
                next.ms >= reset * 1000,
                "a run lookup {} ms before the rate-limit reset followed the 403",
                reset * 1000 - next.ms
            );
            Ok(())
        },
    )
}

pub(super) fn refused_for_good(test: &str) -> TestResult<()> {
    for (tag, replies) in [
        ("T9f", json!([{}, {}, {}, {"status": 403}])),
        ("T9h", json!([{"status": 404}])),
    ] {
        refused(test, tag, &replies)?;
    }
    Ok(())
}

fn refused(test: &str, tag: &'static str, replies: &Value) -> TestResult<()> {
    let mut world = World::new(tag, 1, true)?;
    world.script_refusals(&json!({"run.get": {"after_session": true, "replies": replies}}))?;
    world.queue_message(91, &[push_offer(1, 9001, "main", WORKFLOW)])?;
    world.spawn_controller(test, &[])?;
    world.wait_for_event_unless(
        "message 91 deleted after a permanent source refusal",
        60,
        |_| deleted_messages(&world).contains(&"91".to_owned()),
        |_| {
            world
                .child_stderr()
                .lines()
                .any(|line| line == "source_admission=deferred reason=source_api_unavailable")
                .then(|| format!("{tag}: a permanent source refusal was deferred as retryable"))
        },
    );
    assert!(
        acquire_ids(&world).is_empty(),
        "a refused source check must not acquire"
    );
    world.stop()?;
    assert_eq!(world.finish()?, "Ok(Shutdown)");
    Ok(())
}

fn deferred(
    test: &str,
    tag: &'static str,
    script: &Value,
    after: impl FnOnce(&World) -> TestResult<()>,
) -> TestResult<()> {
    let mut world = World::new(tag, 1, true)?;
    world.script_refusals(script)?;
    world.queue_message(91, &[push_offer(1, 9001, "main", WORKFLOW)])?;
    world.spawn_controller(test, &[])?;
    world.wait_for_event_unless(
        "offer 1 after its check was answered",
        60,
        |_| acquire_ids(&world) == ["1"],
        |events| {
            let answered = events.iter().any(|event| event.label == "acquire");
            let deleted = events
                .iter()
                .any(|event| event.label == "message.delete" && event.field("id") == "91");
            (deleted && !answered)
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
    after(&world)?;
    world.stop()?;
    assert_eq!(world.finish()?, "Ok(Shutdown)");
    assert_clean(&world, "1");
    Ok(())
}
