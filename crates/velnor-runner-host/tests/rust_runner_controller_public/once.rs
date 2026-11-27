// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::time::Duration;

use velnor_runner_host::{Readiness, controller_readiness};

use super::checks::{
    acquire_ids, assert_clean, deleted_messages, jit_name, reservation, runner_name, snapshot,
};
use super::events::{Event, files_containing, wait_for};
use super::fixtures::{TestResult, WORKFLOW, push_offer};
use super::harness::World;

pub(super) fn run(test: &str) -> TestResult<()> {
    let mut world = World::new("T1", 1, false)?;
    world.configure_orb("FAKE_RUNNER_HOLD=1\n")?;
    world.queue_message(
        11,
        &[
            push_offer(1, 1001, "main", WORKFLOW),
            push_offer(2, 1002, "main", WORKFLOW),
        ],
    )?;
    world.spawn_controller(
        test,
        &[
            ("ORBENV", "PAT-CANARY-T1-ORBENV"),
            ("GH_TOKEN", "PAT-CANARY-T1-GHTOKEN"),
        ],
    )?;
    world.wait_for_event("the first jit", 40, |events| {
        events.iter().any(|event| event.label == "jit")
    });
    let first = reservation(&world.gh("jit")[0]);
    holds_the_slot(&world, first)?;
    world.release(&jit_name(first))?;
    after_the_proof(&world, first)?;
    let second = reservation(&world.gh("jit")[1]);
    assert_ne!(first, second);
    world.release(&jit_name(second))?;
    world.wait_for_event("the second runner removal", 40, |events| {
        events.iter().any(|event| {
            event.label == "runner.delete"
                && event.field("name") == runner_name(second)
                && event.field("status") == "204"
        })
    });
    world.stop()?;
    assert_eq!(world.finish(20)?, "Ok(Shutdown)");
    assert_clean(&world, "1");
    assert_canaries(&world, first, second);
    Ok(())
}

fn holds_the_slot(world: &World, first: u64) -> TestResult<()> {
    let name = jit_name(first);
    wait_for("the runner pid", 20, || world.pid_of(&name).is_some());
    let acquire = world.gh("acquire")[0].clone();
    world.wait_for_event_unless(
        "two polls and a redelivery while the slot is held",
        40,
        |events| {
            let polls: Vec<_> = events
                .iter()
                .filter(|event| event.label == "poll" && event.seq > acquire.seq)
                .collect();
            polls.len() >= 2
                && polls
                    .iter()
                    .any(|poll| poll.field("last") == "none" && poll.field("returned") == "11")
        },
        |_| {
            (acquire_ids(world).len() > 1)
                .then(|| "offer 2 was acquired while the only slot was held".to_owned())
        },
    );
    assert_eq!(acquire.field("ids"), "1");
    assert_eq!(world.gh("scaleset.create").len(), 1);
    assert_eq!(world.gh("session.create").len(), 1);
    assert!(
        deleted_messages(world).is_empty(),
        "M1 deleted while offer 2 waits"
    );
    assert_eq!(controller_readiness(&world.state), Readiness::Ready);
    let before = world.heartbeat_ms().ok_or("no heartbeat")?;
    std::thread::sleep(Duration::from_millis(2200));
    assert!(world.heartbeat_ms().ok_or("no heartbeat")? > before);
    assert_eq!(acquire_ids(world), ["1"]);
    let jit_file = world.store.join("jit").join(&name);
    assert_eq!(
        std::fs::read_to_string(&jit_file)?,
        std::fs::read_to_string(world.github.join("jits").join(runner_name(first)))?
    );
    let runner_line = world
        .events()
        .into_iter()
        .find(Event::is_runner_start)
        .ok_or("no runner start")?;
    assert!(
        runner_line.text.contains(&format!(
            "--jit @/var/lib/velnor-host/jit/{first}.jit --work-dir /srv/velnor/gen/work/{first}"
        )),
        "{}",
        runner_line.text
    );
    Ok(())
}

fn after_the_proof(world: &World, first: u64) -> TestResult<()> {
    world.wait_for_event_unless(
        "the first runner removal and the second acquire",
        60,
        |events| {
            events
                .iter()
                .filter(|event| event.label == "acquire")
                .count()
                >= 2
                && events.iter().any(|event| event.label == "message.delete")
        },
        |events| {
            let acquired: Vec<&str> = events
                .iter()
                .filter(|event| event.label == "acquire")
                .map(|event| event.field("ids"))
                .collect();
            let mut distinct = acquired.clone();
            distinct.sort_unstable();
            distinct.dedup();
            (distinct.len() != acquired.len())
                .then(|| format!("a runner request was acquired twice: {acquired:?}"))
        },
    );
    let events = world.events();
    let removal = events
        .iter()
        .find(|event| event.label == "runner.delete" && event.field("name") == runner_name(first))
        .ok_or("no first runner removal")?;
    let acquires: Vec<_> = events
        .iter()
        .filter(|event| event.label == "acquire")
        .collect();
    assert_eq!(acquire_ids(world), ["1", "2"]);
    assert!(acquires[1].seq > removal.seq);
    let message = events
        .iter()
        .find(|event| event.label == "message.delete")
        .ok_or("no delete")?;
    assert_eq!(message.field("id"), "11");
    assert!(message.seq > acquires[1].seq);
    assert!(
        events.iter().any(|event| event.label == "poll"
            && event.seq > removal.seq
            && event.field("last") == "none"
            && event.field("returned") == "11"),
        "M1 was not redelivered after the proof"
    );
    let state = snapshot(world, acquires[1]);
    assert!(!state.jit.contains(&jit_name(first)), "{:?}", state.jit);
    assert!(!state.work.contains(&first.to_string()), "{:?}", state.work);
    Ok(())
}

fn assert_canaries(world: &World, first: u64, second: u64) {
    assert_eq!(world.gh("jit").len(), 2);
    assert!(
        files_containing(&world.root, "PAT-CANARY-T1")
            .iter()
            .all(|path| path.ends_with("pat")),
        "{:?}",
        files_containing(&world.root, "PAT-CANARY-T1")
    );
    for path in files_containing(&world.root, "JIT-CANARY-T1-") {
        let name = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("");
        let allowed = path.starts_with(world.github.join("jits"))
            || name == jit_name(first)
            || name == jit_name(second);
        assert!(
            allowed && path.starts_with(&world.root),
            "{}",
            path.display()
        );
        assert!(!path.starts_with(&world.store), "{}", path.display());
        assert!(!path.starts_with(&world.state), "{}", path.display());
    }
}
