// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::checks::{acquire_ids, assert_clean, deleted_messages, reservation, runner_name};
use super::events::Event;
use serde_json::json;

use super::fixtures::{
    OTHER_WORKFLOW, TestResult, WORKFLOW, pull_offer, push_offer, sha, with_acquire_none,
    with_head_repository, with_run_repository,
};
use super::harness::World;

const REPOSITORY_READS: usize = 8;

pub(super) fn run(test: &str) -> TestResult<()> {
    let mut world = World::new("T2", 1, true)?;
    world.write_config("trunk")?;
    world.script_refusals(&json!({
        "repo.get": {"after_session": true, "replies": [{}, {"patch": {"id": 43}}, {}, {"patch": {"default_branch": null}}]}
    }))?;
    world.queue_message(19, &[push_offer(6, 2006, "main", WORKFLOW)])?;
    world.queue_message(20, &[push_offer(7, 2007, "main", WORKFLOW)])?;
    world.queue_message(21, &[push_offer(1, 2001, "main", OTHER_WORKFLOW)])?;
    world.queue_message(22, &[pull_offer(2, 2002, 7, Some(&[sha(1), sha(2)]))])?;
    world.queue_message(23, &[push_offer(3, 2003, "not-allowed", WORKFLOW)])?;
    world.queue_message(
        24,
        &[with_acquire_none(push_offer(5, 2005, "main", WORKFLOW))],
    )?;
    let fork = with_head_repository(pull_offer(8, 2008, 9, None), 43);
    world.queue_message(25, &[fork])?;
    let other = with_run_repository(push_offer(9, 2009, "main", WORKFLOW), 43);
    world.queue_message(26, &[other])?;
    world.queue_message(27, &[pull_offer(4, 2004, 8, None)])?;
    world.spawn_controller(test, &[])?;
    world.wait_for_event("the deletes of the refused messages", 60, |events| {
        ["19", "20", "21", "22", "23", "24", "25", "26"]
            .iter()
            .all(|id| {
                events
                    .iter()
                    .any(|event| event.label == "message.delete" && event.field("id") == *id)
            })
    });
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
    let opened = world.gh("session.create")[0].seq;
    let fetches = world
        .gh("repo.get")
        .iter()
        .filter(|event| event.seq > opened)
        .count();
    assert_eq!(fetches, REPOSITORY_READS);
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
