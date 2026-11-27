// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::checks::{acquire_ids, assert_clean, deleted_messages, reservation, runner_name};
use super::events::Event;
use serde_json::json;

use super::fixtures::{
    OTHER_WORKFLOW, TestResult, WORKFLOW, merge_group_host_toml, merge_group_offer, pull_offer,
    push_offer, sha, with_acquire_none, with_head_repository, with_run_repository,
};
use super::harness::World;

const FIRST_PHASE_READS: [(&str, usize); 3] = [("m19", 2), ("m20", 2), ("m24", 2)];

fn feed(world: &World, phase: &str, message: u64, offer: serde_json::Value) -> TestResult<()> {
    std::fs::write(world.seen.join("phase"), phase)?;
    world.queue_message(message, &[offer])?;
    let id = message.to_string();
    world.wait_for_event("the delete of the refused message", 60, |events| {
        events
            .iter()
            .any(|event| event.label == "message.delete" && event.field("id") == id)
    });
    Ok(())
}

pub(super) fn run(test: &str) -> TestResult<()> {
    let mut world = World::new("T2", 1, true)?;
    world.write_config("trunk")?;
    world.script_refusals(&json!({
        "repo.get@m19": {"after_session": true, "replies": [{}, {"patch": {"id": 43}}]},
        "repo.get@m20": {"after_session": true, "replies": [{}, {"patch": {"default_branch": null}}]}
    }))?;
    world.spawn_controller(test, &[])?;
    world.wait_for_event("the session", 60, |events| {
        events.iter().any(|event| event.label == "session.create")
    });
    feed(&world, "m19", 19, push_offer(6, 2006, "main", WORKFLOW))?;
    feed(&world, "m20", 20, push_offer(7, 2007, "main", WORKFLOW))?;
    feed(
        &world,
        "m21",
        21,
        push_offer(1, 2001, "main", OTHER_WORKFLOW),
    )?;
    feed(
        &world,
        "m22",
        22,
        pull_offer(2, 2002, 7, Some(&[sha(1), sha(2)])),
    )?;
    feed(
        &world,
        "m23",
        23,
        push_offer(3, 2003, "not-allowed", WORKFLOW),
    )?;
    feed(
        &world,
        "m24",
        24,
        with_acquire_none(push_offer(5, 2005, "main", WORKFLOW)),
    )?;
    feed(
        &world,
        "m25",
        25,
        with_head_repository(pull_offer(8, 2008, 9, None), 43),
    )?;
    feed(
        &world,
        "m26",
        26,
        with_run_repository(push_offer(9, 2009, "main", WORKFLOW), 43),
    )?;
    std::fs::write(world.seen.join("phase"), "m27")?;
    world.queue_message(27, &[pull_offer(4, 2004, 8, None)])?;
    world.wait_for_event("the sentinel runner and its removal", 60, |events| {
        events
            .iter()
            .any(|event| event.label == "runner.delete" && event.field("status") == "204")
    });
    world.stop()?;
    assert_eq!(world.finish(20)?, "Ok(Shutdown)");
    assert_outcome(&world);
    assert_clean(&world, "1");
    Ok(())
}

fn reads_in(world: &World, phase: &str) -> usize {
    world
        .gh("repo.get")
        .iter()
        .filter(|event| event.field("phase") == phase)
        .count()
}

fn assert_outcome(world: &World) {
    let blocked: Vec<String> = world
        .child_stderr()
        .lines()
        .filter(|line| line.starts_with("source_admission=blocked reason="))
        .map(str::to_owned)
        .collect();
    assert_eq!(
        blocked,
        [
            "source_admission=blocked reason=identity",
            "source_admission=blocked reason=identity",
            "source_admission=blocked reason=identity",
            "source_admission=blocked reason=identity",
            "source_admission=blocked reason=ref_denied",
            "source_admission=blocked reason=cross_repository",
            "source_admission=blocked reason=cross_repository",
        ]
    );
    assert_eq!(acquire_ids(world), ["5", "4"]);
    let deleted = deleted_messages(world);
    for id in ["19", "20", "21", "22", "23", "24", "25", "26"] {
        assert!(
            deleted.contains(&id.to_owned()),
            "{id} not acknowledged: {deleted:?}"
        );
    }
    let jits = world.gh("jit");
    assert_eq!(jits.len(), 1, "only the sentinel gets a JIT");
    for (phase, reads) in FIRST_PHASE_READS {
        assert_eq!(
            reads_in(world, phase),
            reads,
            "phase {phase}: the repository must be read once to authorize and once for the default branch"
        );
    }
    assert_eq!(
        reads_in(world, "m27"),
        1,
        "the accepted offer after a fetched default branch must be served from the cache"
    );
    assert!(
        world
            .child_stderr()
            .lines()
            .any(|line| line.starts_with("rust_runner_job=ended")
                && line.contains("default_branch=trunk")),
        "the grant carries the default branch of GET /repos"
    );
    let runners: Vec<_> = world
        .events()
        .into_iter()
        .filter(Event::is_runner_start)
        .collect();
    assert_eq!(runners.len(), 1);
    let sentinel = reservation(&jits[0]);
    assert!(
        runners[0]
            .text
            .contains(&format!("/var/lib/velnor/jit/{sentinel}.jit")),
        "{}",
        runners[0].text
    );
    assert_eq!(jits[0].field("name"), runner_name(sentinel));
}

pub(super) fn merge_groups(test: &str) -> TestResult<()> {
    let mut world = World::new("T2b", 1, true)?;
    world.write_host_toml_text(&merge_group_host_toml(&world.root)?)?;
    let queue = |pull: u64, run: u64| format!("gh-readonly-queue/main/pr-{pull}-{}", sha(run));
    world.queue_message(
        31,
        &[merge_group_offer(
            1,
            3001,
            &format!("refs/heads/{}", queue(3, 3001)),
            &queue(3, 3001),
        )],
    )?;
    world.queue_message(
        32,
        &[merge_group_offer(
            2,
            3002,
            "refs/merge-queue/main/pr-4",
            &queue(4, 3002),
        )],
    )?;
    world.spawn_controller(test, &[])?;
    world.wait_for_event("both merge group offers", 60, |_| {
        acquire_ids(&world) == ["1", "2"]
    });
    assert!(
        !world
            .child_stderr()
            .lines()
            .any(|line| line.starts_with("source_admission=blocked")),
        "a merge group offer was refused"
    );
    world.stop()?;
    assert_eq!(world.finish(30)?, "Ok(Shutdown)");
    assert_clean(&world, "1");
    Ok(())
}
