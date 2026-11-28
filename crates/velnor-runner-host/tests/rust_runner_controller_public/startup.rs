// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde_json::json;

use super::checks::heartbeat_advances;
use super::child::HOLD_STORE;
use super::events::wait_for;
use super::fixtures::TestResult;
use super::harness::World;

pub(super) fn run(test: &str) -> TestResult<()> {
    let mut world = World::new("T13", 1, true)?;
    world.script_refusals(&json!({"scaleset.get": {"replies": [{"status": 503}]}}))?;
    world.spawn_controller(test, &[])?;
    world.wait_for_event("the held lookup of the scale set", 60, |events| {
        events
            .iter()
            .any(|event| event.label == "scaleset.get" && event.field("status") == "503")
    });
    world.write_host_toml(2)?;
    assert_eq!(
        world.finish(20)?,
        "Ok(ConfigChanged)",
        "a held startup must notice a changed host.toml before its 30 s retry"
    );
    assert_eq!(world.gh("scaleset.get").len(), 1);
    assert!(world.gh("session.create").is_empty());
    Ok(())
}

pub(super) fn waits_while_the_store_is_held(test: &str) -> TestResult<()> {
    let mut world = World::new("T13b", 1, true)?;
    world.spawn_controller(test, &[(HOLD_STORE, "1")])?;
    wait_for(
        "the startup heartbeat of a controller that waits for the store",
        40,
        || world.heartbeat_phase().as_deref() == Some("startup"),
    );
    heartbeat_advances(&world, 2);
    assert!(
        world.events().is_empty(),
        "a controller started while another one held the host store: {:?}",
        world.events()
    );
    assert!(
        world
            .child_stderr()
            .contains("rust-runner startup held: another controller holds the host store")
    );
    world.stop()?;
    assert_eq!(world.finish(30)?, "Ok(Shutdown)");
    Ok(())
}

pub(super) fn holds_a_changed_config_until_the_session_is_closed(test: &str) -> TestResult<()> {
    let mut world = World::new("T13c", 1, true)?;
    world.script_refusals(&json!({
        "runner.list": {"statuses": [500], "repeat_last": true},
        "session.delete": {"statuses": [500, 404], "repeat_last": false}
    }))?;
    world.spawn_controller(test, &[])?;
    world.wait_for_event("the session of a held startup", 60, |events| {
        events.iter().any(|event| event.label == "session.create")
    });
    world.write_host_toml(2)?;
    assert_eq!(world.finish(90)?, "Ok(ConfigChanged)");
    let closes: Vec<String> = world
        .gh("session.delete")
        .iter()
        .map(|event| event.field("status").to_owned())
        .collect();
    assert_eq!(
        closes,
        ["500", "404", "204"],
        "a startup left before a 204 confirmed the close of its session"
    );
    assert!(
        world.child_stderr().contains("session close held"),
        "{}",
        world.child_stderr()
    );
    Ok(())
}

pub(super) fn counts_a_404_after_a_timed_out_close_as_closed(test: &str) -> TestResult<()> {
    let mut world = World::new("T13d", 1, true)?;
    world.script_refusals(&json!({
        "runner.list": {"statuses": [500], "repeat_last": true},
        "session.stall": {"statuses": [12000], "repeat_last": false},
        "session.delete": {"statuses": [404, 404], "repeat_last": false}
    }))?;
    world.spawn_controller(test, &[])?;
    world.wait_for_event("the session of a held startup", 60, |events| {
        events.iter().any(|event| event.label == "session.create")
    });
    world.write_host_toml(2)?;
    assert_eq!(world.finish(90)?, "Ok(ConfigChanged)");
    let closes: Vec<String> = world
        .gh("session.delete")
        .iter()
        .map(|event| event.field("status").to_owned())
        .collect();
    assert_eq!(
        closes,
        ["404", "404"],
        "a 404 after a close that timed out must count as closed"
    );
    Ok(())
}
