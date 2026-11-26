// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::os::unix::fs::PermissionsExt;
use std::time::Instant;

use serde_json::json;
use velnor_runner_host::{Readiness, controller_readiness};

use super::checks::{assert_clean, reservation, snapshot};
use super::events::{Event, wait_for};
use super::fixtures::{TestResult, WORKFLOW, push_offer};
use super::harness::World;

const STALE_RUNNER: &str = "4242 /var/lib/velnor/bin/velnor-runner run --jit @/var/lib/velnor/jit/99.jit --work-dir /var/lib/velnor/work/99\n4343 sleep 600\n";

fn move_logs_aside(world: &World, run: &str) -> TestResult<()> {
    for name in ["requests", "seq"] {
        let from = world.seen.join(name);
        if from.exists() {
            std::fs::rename(&from, world.seen.join(format!("{name}.{run}")))?;
        }
    }
    let snapshots = world.seen.join("snapshots");
    if snapshots.exists() {
        std::fs::rename(snapshots, world.seen.join(format!("snapshots.{run}")))?;
    }
    let _ = std::fs::remove_file(world.seen.join("stop"));
    Ok(())
}

pub(super) fn run(test: &str) -> TestResult<()> {
    let mut world = World::new("T4", 1, true)?;
    std::fs::write(world.root.join("ps.table"), STALE_RUNNER)?;
    std::fs::create_dir_all(world.store.join("jit"))?;
    std::fs::set_permissions(
        world.store.join("jit"),
        std::fs::Permissions::from_mode(0o700),
    )?;
    std::fs::create_dir_all(world.store.join("work/99"))?;
    std::fs::write(world.store.join("jit/99.jit"), "stale")?;
    std::fs::set_permissions(
        world.store.join("jit/99.jit"),
        std::fs::Permissions::from_mode(0o600),
    )?;
    std::fs::write(world.store.join("work/99/leftover"), "stale")?;
    world.write_state(true, &[("velnor-mac-99", 90)])?;
    world.configure_orb("FAKE_ORB_PS_IMMORTAL=4242\n")?;
    world.queue_message(41, &[push_offer(1041, 1041, "main", WORKFLOW)])?;
    unconfirmed_sweep(&mut world, test)?;
    move_logs_aside(&world, "run1")?;
    resumed_startup(&mut world, test)?;
    move_logs_aside(&world, "run2")?;
    shutdown_with_a_stuck_removal(&mut world, test)?;
    assert_clean(&world, "1");
    Ok(())
}

fn unconfirmed_sweep(world: &mut World, test: &str) -> TestResult<()> {
    world.spawn_controller(test, &[])?;
    world.wait_for_event("the kill and the confirming ps", 30, |events| {
        let kill = events.iter().find(|event| event.is_orb("kill -9 4242"));
        kill.is_some_and(|kill| {
            events
                .iter()
                .any(|event| event.seq > kill.seq && event.is_orb("ps -eo"))
        })
    });
    let started = world.heartbeat_ms().unwrap_or_default();
    wait_for("two heartbeats in the startup phase", 20, || {
        world.heartbeat_ms().unwrap_or_default() > started + 3000
    });
    assert_eq!(world.heartbeat_phase().as_deref(), Some("startup"));
    for label in [
        "session.create",
        "poll",
        "acquire",
        "message.delete",
        "scaleset.get",
    ] {
        assert!(
            world.gh(label).is_empty(),
            "{label} before the sweep was proven"
        );
    }
    assert_eq!(controller_readiness(&world.state), Readiness::Reconciling);
    assert!(
        world
            .events()
            .iter()
            .all(|event| !event.is_orb("kill -9 4343"))
    );
    let stopped = Instant::now();
    world.stop()?;
    assert_eq!(world.finish(10)?, "Ok(Shutdown)");
    assert!(stopped.elapsed().as_secs() < 10);
    assert!(world.gh("session.delete").is_empty());
    Ok(())
}

fn resumed_startup(world: &mut World, test: &str) -> TestResult<()> {
    std::fs::write(world.seen.join("release"), "")?;
    world.script_refusals(&json!({"velnor-mac-99": {"statuses": [400], "repeat_last": false}}))?;
    world.spawn_controller(test, &[])?;
    world.wait_for_event("the first job and its removal", 90, |events| {
        let acquired = events.iter().any(|event| event.label == "acquire");
        acquired
            && events.iter().any(|event| {
                event.label == "runner.delete"
                    && event.field("status") == "204"
                    && event.field("name") != "velnor-mac-99"
            })
    });
    let events = world.events();
    let kill = events
        .iter()
        .find(|event| event.is_orb("kill -9 4242"))
        .ok_or("no kill")?;
    let session = events
        .iter()
        .find(|event| event.label == "session.create")
        .ok_or("no session")?;
    let deletes: Vec<&Event> = events
        .iter()
        .filter(|event| event.label == "runner.delete" && event.field("name") == "velnor-mac-99")
        .collect();
    let acquire = events
        .iter()
        .find(|event| event.label == "acquire")
        .ok_or("no acquire")?;
    assert!(kill.seq < session.seq && session.seq < deletes[0].seq && deletes[0].seq < acquire.seq);
    assert_eq!(
        world.gh("session.create").len(),
        1,
        "the retry recreated the session"
    );
    assert_eq!(deletes.len(), 2);
    assert_eq!(deletes[0].field("status"), "400");
    assert_eq!(deletes[1].field("status"), "204");
    assert!(
        deletes[1].ms >= deletes[0].ms + 25_000,
        "{} then {}",
        deletes[0].ms,
        deletes[1].ms
    );
    let empty = events
        .iter()
        .find(|event| {
            event.label == "runner.list"
                && event.seq > deletes[1].seq
                && event.field("names").is_empty()
        })
        .ok_or("no empty list after the removal")?;
    assert!(acquire.seq > empty.seq);
    assert!(
        events
            .iter()
            .all(|event| event.label != "message.delete" || event.seq > empty.seq)
    );
    let table = std::fs::read_to_string(world.root.join("ps.table"))?;
    assert!(
        table.contains("4343 sleep 600") && !table.contains("4242 "),
        "{table}"
    );
    let state = snapshot(world, acquire);
    assert!(
        state
            .jit
            .iter()
            .chain(&state.work)
            .all(|name| !name.contains("99")),
        "{state_jit:?}",
        state_jit = state.jit
    );
    let own = reservation(&world.gh("jit")[0]);
    assert!(state.jit.iter().all(|name| name == &format!("{own}.jit")) || state.jit.is_empty());
    world.stop()?;
    assert_eq!(world.finish(10)?, "Ok(Shutdown)");
    Ok(())
}

fn shutdown_with_a_stuck_removal(world: &mut World, test: &str) -> TestResult<()> {
    world.write_state(true, &[("velnor-mac-98", 80)])?;
    world.script_refusals(&json!({"velnor-mac-98": {"statuses": [400], "repeat_last": true}}))?;
    world.queue_message(42, &[push_offer(1042, 1042, "main", WORKFLOW)])?;
    world.spawn_controller(test, &[])?;
    world.wait_for_event("the refused removal of velnor-mac-98", 60, |events| {
        events
            .iter()
            .any(|event| event.label == "runner.delete" && event.field("name") == "velnor-mac-98")
    });
    let stopped = Instant::now();
    world.stop()?;
    assert_eq!(world.finish(10)?, "Ok(Shutdown)");
    assert!(stopped.elapsed().as_secs() < 10);
    let events = world.events();
    let refused = events
        .iter()
        .find(|event| event.label == "runner.delete" && event.field("name") == "velnor-mac-98")
        .ok_or("no refusal")?;
    assert!(
        events
            .iter()
            .any(|event| event.label == "session.delete" && event.seq > refused.seq)
    );
    assert!(
        events
            .iter()
            .all(|event| event.label != "poll" && event.label != "acquire")
    );
    Ok(())
}
