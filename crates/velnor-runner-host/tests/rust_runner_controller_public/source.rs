// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::checks::{acquire_ids, assert_clean, deleted_messages, reservation, runner_name};
use super::events::Event;
use super::fixtures::{
    OTHER_WORKFLOW, TestResult, WORKFLOW, pull_offer, push_offer, sha, with_acquire_none,
};
use super::harness::World;

pub(super) fn run(test: &str) -> TestResult<()> {
    let mut world = World::new("T2", 1, true)?;
    world.queue_message(21, &[push_offer(1, 2001, "main", OTHER_WORKFLOW)])?;
    world.queue_message(22, &[pull_offer(2, 2002, 7, Some(&[sha(1), sha(2)]))])?;
    world.queue_message(23, &[push_offer(3, 2003, "not-allowed", WORKFLOW)])?;
    world.queue_message(
        24,
        &[with_acquire_none(push_offer(5, 2005, "main", WORKFLOW))],
    )?;
    world.queue_message(25, &[pull_offer(4, 2004, 8, None)])?;
    world.spawn_controller(test, &[])?;
    world.wait_for_event("the deletes of the four refused messages", 60, |events| {
        ["21", "22", "23", "24"].iter().all(|id| {
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
            "source_admission=blocked reason=ref_denied",
        ]
    );
    assert_eq!(acquire_ids(world), ["5", "4"]);
    let deleted = deleted_messages(world);
    for id in ["21", "22", "23", "24"] {
        assert!(
            deleted.contains(&id.to_owned()),
            "{id} not acknowledged: {deleted:?}"
        );
    }
    let jits = world.gh("jit");
    assert_eq!(jits.len(), 1, "only the sentinel gets a JIT");
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
