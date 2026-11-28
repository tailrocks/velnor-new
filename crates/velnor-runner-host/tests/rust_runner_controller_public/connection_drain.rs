// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::time::Duration;

use velnor_runner_host::{
    ConnectionId, wait_for_rust_runner_connection_admitted, wait_for_rust_runner_connection_drain,
};

use super::child::SUPERVISED;
use super::events::wait_for;
use super::fixtures::TestResult;
use super::harness::World;
use super::world_files::{TOKEN_A, TOKEN_B};

fn admitting(world: &World, expected: bool) -> bool {
    world
        .census_tally("lab")
        .is_some_and(|tally| tally["admitting"] == expected && tally["drain"].is_null())
}

pub(super) fn run(test: &str) -> TestResult<()> {
    let mut world = World::new("C6", 1, true)?;
    let id = ConnectionId::parse("lab")?;
    world.spawn_controller(test, &[(SUPERVISED, "1")])?;
    wait_for("the census to admit the connection", 60, || {
        admitting(&world, true)
    });

    let directory = world.state.join("connection-drain");
    std::fs::write(&directory, "not a directory")?;
    wait_for(
        "admission closed by a marker path that cannot be read",
        30,
        || admitting(&world, false),
    );
    std::fs::remove_file(&directory)?;
    wait_for("admission reopened", 30, || admitting(&world, true));

    world.write_marker("connection-drain/lab", TOKEN_A)?;
    wait_for("the census to record the drained connection", 30, || {
        world.census_tally("lab").is_some_and(|tally| {
            tally["slots"] == 0 && tally["drain"] == TOKEN_A && tally["admitting"] == false
        })
    });
    assert!(wait_for_rust_runner_connection_drain(
        &world.state,
        &id,
        Duration::from_secs(20)
    )?);
    assert!(!wait_for_rust_runner_connection_admitted(
        &world.state,
        &id,
        Duration::from_secs(3)
    )?);

    std::fs::remove_file(world.state.join("connection-drain/lab"))?;
    assert!(wait_for_rust_runner_connection_admitted(
        &world.state,
        &id,
        Duration::from_secs(30)
    )?);

    world.write_marker("connection-drain/lab", TOKEN_A)?;
    wait_for("the marker to close admission again", 30, || {
        admitting(&world, false)
            || world
                .census_tally("lab")
                .is_some_and(|t| t["drain"] == TOKEN_A)
    });
    world.stop()?;
    assert_eq!(world.finish(20)?, "Ok(Stopped)");
    assert!(wait_for_rust_runner_connection_drain(
        &world.state,
        &id,
        Duration::from_secs(2)
    )?);
    world.write_marker("connection-drain/lab", TOKEN_B)?;
    assert!(!wait_for_rust_runner_connection_drain(
        &world.state,
        &id,
        Duration::from_secs(1)
    )?);
    world.write_marker("connection-drain/lab", "not a token")?;
    assert!(!wait_for_rust_runner_connection_drain(
        &world.state,
        &id,
        Duration::from_secs(1)
    )?);
    Ok(())
}
