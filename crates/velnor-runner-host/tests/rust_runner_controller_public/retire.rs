// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde_json::{Value, json};

use super::checks::{heartbeat_advances, jit_name};
use super::child::SUPERVISED;
use super::events::wait_for;
use super::fixtures::{
    KEYCHAIN_REFERENCE, Spec, TestResult, WORKFLOW, host_toml, host_toml_of, push_offer,
};
use super::harness::World;

const STALL_MS: u64 = 9_000;
const CLOSE_TIMEOUT_MS: u64 = 12_000;
const TICK_GAP_LIMIT_MS: u64 = 7_000;

fn session_deletes(world: &World, set: &str) -> Vec<String> {
    world
        .gh("session.delete")
        .iter()
        .filter(|event| event.field("set") == set)
        .map(|event| event.field("status").to_owned())
        .collect()
}

fn closes_do_not_overlap(world: &World, set: &str) -> bool {
    let mut open = false;
    for event in world.events() {
        if event.field("set") != set {
            continue;
        }
        match event.label.as_str() {
            "session.delete.begin" if open => return false,
            "session.delete.begin" => open = true,
            "session.delete" => open = false,
            _ => {}
        }
    }
    true
}

fn longest_census_gap_until(world: &World, what: &str, mut done: impl FnMut() -> bool) -> u64 {
    let (mut last, mut longest) = (0_u64, 0_u64);
    wait_for(what, 120, || {
        let stamp = world
            .census()
            .and_then(|census| census["updated_unix_ms"].as_u64())
            .unwrap_or_default();
        if stamp > last {
            if last > 0 {
                longest = longest.max(stamp - last);
            }
            last = stamp;
        }
        done()
    });
    longest
}

pub(super) fn removal(test: &str) -> TestResult<()> {
    let (lab, second) = (Spec::lab(KEYCHAIN_REFERENCE), Spec::second());
    let mut world = World::with_extra("R5", 3, false, &[second])?;
    let both = host_toml_of(&world.root, 3, &[lab, second])?;
    let only_second = host_toml_of(&world.root, 3, &[second])?;
    world.write_host_toml_text(&both)?;
    world.configure_orb("FAKE_RUNNER_HOLD=1\n")?;
    world.queue_message(51, &[push_offer(1, 5101, "main", WORKFLOW)])?;
    world.spawn_controller(test, &[(SUPERVISED, "1")])?;
    let job = world.held_job(1);

    world.write_host_toml_text(&only_second)?;
    world.wait_census(
        "the config without lab applied while its job runs",
        &only_second,
        &["lab", "second"],
    );
    heartbeat_advances(&world, 3);
    assert_eq!(world.census_tally("lab").ok_or("tally")?["slots"], 1);
    assert_eq!(session_deletes(&world, "7"), Vec::<String>::new());
    assert!(world.pid_of(&jit_name(job)).is_some());

    world.script_phase(&json!({
        "session.stall": {"statuses": [STALL_MS], "repeat_last": false},
        "session.delete": {"statuses": [404, 500], "repeat_last": false}
    }))?;
    world.release(&jit_name(job))?;
    let longest =
        longest_census_gap_until(&world, "the second failed close of the session", || {
            session_deletes(&world, "7").len() >= 2
        });
    assert!(
        longest < TICK_GAP_LIMIT_MS,
        "the census stood still for {longest} ms while the session close was stalled"
    );
    assert!(
        world.census_tally("lab").is_some(),
        "a failed close must keep the connection in the census"
    );

    world.wait_census("the census without lab", &only_second, &["second"]);
    assert_eq!(session_deletes(&world, "7"), ["404", "500", "204"]);
    assert!(
        closes_do_not_overlap(&world, "7"),
        "a second close started while the first was in flight"
    );
    assert_eq!(session_deletes(&world, "8"), Vec::<String>::new());

    world.stop()?;
    assert_eq!(world.finish(20)?, "Ok(Stopped)");
    Ok(())
}

pub(super) fn restart(test: &str) -> TestResult<()> {
    let (lab, second) = (Spec::lab(KEYCHAIN_REFERENCE), Spec::second());
    let mut world = World::with_extra("R6", 3, false, &[second])?;
    let both = host_toml_of(&world.root, 3, &[lab, second])?;
    let narrowed = host_toml_of(&world.root, 2, &[second])?;
    world.write_host_toml_text(&both)?;
    world.spawn_controller(test, &[(SUPERVISED, "1")])?;
    world.wait_census("both connections serving", &both, &["lab", "second"]);

    world.script_phase(&json!({
        "session.delete": {"statuses": [500, 404], "repeat_last": false}
    }))?;
    world.write_host_toml_text(&narrowed)?;
    world.wait_census("the census of the narrowed config", &narrowed, &["second"]);
    assert_eq!(
        session_deletes(&world, "7"),
        ["500", "404", "204"],
        "the narrowed config proved lab unserved before its session was closed"
    );
    world.wait_for_event(
        "the second connection to open a new session",
        60,
        |events| {
            events
                .iter()
                .filter(|event| event.label == "session.create" && event.field("set") == "8")
                .count()
                >= 2
        },
    );

    world.stop()?;
    assert_eq!(world.finish(20)?, "Ok(Stopped)");
    Ok(())
}

pub(super) fn revert(test: &str) -> TestResult<()> {
    let (lab, second) = (Spec::lab(KEYCHAIN_REFERENCE), Spec::second());
    let mut world = World::with_extra("R7", 3, false, &[second])?;
    let both = host_toml_of(&world.root, 3, &[lab, second])?;
    let narrowed = host_toml_of(&world.root, 2, &[second])?;
    world.write_host_toml_text(&both)?;
    world.spawn_controller(test, &[(SUPERVISED, "1")])?;
    world.wait_census("both connections serving", &both, &["lab", "second"]);

    world.script_phase(&json!({"session.delete": {"statuses": [500], "repeat_last": false}}))?;
    world.write_host_toml_text(&narrowed)?;
    world.wait_for_event("the first failed close of lab", 60, |events| {
        events
            .iter()
            .any(|event| event.label == "session.delete" && event.field("set") == "7")
    });
    world.write_host_toml_text(&both)?;
    world.wait_for_event("lab to serve again on a new session", 90, |events| {
        events
            .iter()
            .filter(|event| event.label == "session.create" && event.field("set") == "7")
            .count()
            >= 2
    });
    world.wait_census("both connections serving again", &both, &["lab", "second"]);
    wait_for("the second close of the reverted session", 30, || {
        session_deletes(&world, "7").len() >= 2
    });
    assert_eq!(session_deletes(&world, "7")[..2], ["500", "204"]);

    world.stop()?;
    assert_eq!(world.finish(20)?, "Ok(Stopped)");
    Ok(())
}

fn retiring_alone(test: &str, tag: &'static str, script: &Value) -> TestResult<(World, String)> {
    let mut world = World::new(tag, 1, false)?;
    let with_lab = host_toml(&world.root, 1)?;
    let without = host_toml_of(&world.root, 1, &[])?;
    world.spawn_controller(test, &[(SUPERVISED, "1")])?;
    world.wait_census("lab serving", &with_lab, &["lab"]);
    world.script_phase(script)?;
    world.write_host_toml_text(&without)?;
    Ok((world, without))
}

fn retired_alone(test: &str, tag: &'static str, script: &Value) -> TestResult<Vec<String>> {
    let (mut world, without) = retiring_alone(test, tag, script)?;
    world.wait_census("the census without lab", &without, &[]);
    let closes = session_deletes(&world, "7");
    world.stop()?;
    assert_eq!(world.finish(30)?, "Ok(Stopped)");
    Ok(closes)
}

pub(super) fn closes_after_a_timeout_and_a_404(test: &str) -> TestResult<()> {
    let closes = retired_alone(
        test,
        "R8",
        &json!({
            "session.stall": {"statuses": [CLOSE_TIMEOUT_MS], "repeat_last": false},
            "session.delete": {"statuses": [404], "repeat_last": false}
        }),
    )?;
    assert_eq!(
        closes,
        ["404"],
        "a 404 after a retired close that timed out must count as closed"
    );
    Ok(())
}

pub(super) fn holds_a_first_attempt_404(test: &str) -> TestResult<()> {
    let closes = retired_alone(
        test,
        "R9",
        &json!({"session.delete": {"statuses": [404], "repeat_last": false}}),
    )?;
    assert_eq!(
        closes,
        ["404", "204"],
        "a 404 on the first close of a retired connection must be held"
    );
    Ok(())
}

fn stopped_after_retiring(test: &str, tag: &'static str, script: &Value) -> TestResult<World> {
    let (mut world, _) = retiring_alone(test, tag, script)?;
    world.wait_for_event("the second close of the retired session", 90, |events| {
        events
            .iter()
            .filter(|event| event.label == "session.delete.begin")
            .count()
            >= 2
    });
    world.stop()?;
    assert_eq!(world.finish(30)?, "Ok(Stopped)");
    Ok(world)
}

pub(super) fn stop_counts_a_404_after_a_timed_out_close_as_closed(test: &str) -> TestResult<()> {
    let world = stopped_after_retiring(
        test,
        "S1",
        &json!({
            "session.stall": {"statuses": [CLOSE_TIMEOUT_MS, CLOSE_TIMEOUT_MS], "repeat_last": false},
            "session.delete": {"statuses": [404], "repeat_last": false}
        }),
    )?;
    assert_eq!(session_deletes(&world, "7"), ["404"]);
    assert_eq!(
        world.child_stderr().matches("session close held").count(),
        1,
        "the stop treated a 404 after a timed out close as held: {}",
        world.child_stderr()
    );
    Ok(())
}

pub(super) fn stop_holds_a_first_attempt_404(test: &str) -> TestResult<()> {
    let mut world = World::new("S2", 1, false)?;
    let with_lab = host_toml(&world.root, 1)?;
    world.spawn_controller(test, &[(SUPERVISED, "1")])?;
    world.wait_census("lab serving", &with_lab, &["lab"]);
    world.script_phase(&json!({"session.delete": {"statuses": [404], "repeat_last": false}}))?;
    world.stop()?;
    assert_eq!(world.finish(30)?, "Ok(Stopped)");
    assert_eq!(session_deletes(&world, "7"), ["404"]);
    assert_eq!(
        world.child_stderr().matches("session close held").count(),
        1,
        "the stop treated a first-attempt 404 as closed: {}",
        world.child_stderr()
    );
    Ok(())
}
