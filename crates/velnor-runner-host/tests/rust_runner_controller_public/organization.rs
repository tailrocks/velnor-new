// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde_json::Value;

use super::checks::{assert_clean, heartbeat_advances};
use super::fixtures::TestResult;
use super::harness::World;

const RECORDS: &str = "rust-runner-scale-sets.json";

fn recorded(world: &World) -> TestResult<Value> {
    Ok(serde_json::from_str(&std::fs::read_to_string(
        world.state.join(RECORDS),
    )?)?)
}

fn started(world: &mut World, test: &str) -> TestResult<()> {
    world.use_organization()?;
    world.spawn_controller(test, &[])?;
    Ok(())
}

pub(super) fn run(test: &str) -> TestResult<()> {
    adopts_a_matching_set_the_record_does_not_list(test)?;
    creates_and_records_a_missing_set(test)?;
    refuses_a_set_the_record_assigns_to_another_id(test)
}

fn adopts_a_matching_set_the_record_does_not_list(test: &str) -> TestResult<()> {
    let mut world = World::new("T12a", 1, true)?;
    started(&mut world, test)?;
    world.wait_for_event("the session of an adopted set", 60, |events| {
        events.iter().any(|event| event.label == "session.create")
    });
    assert!(world.gh("scaleset.create").is_empty());
    assert_eq!(
        recorded(&world)?["lab"],
        7,
        "the adopted set must be recorded"
    );
    world.stop()?;
    assert_eq!(world.finish(30)?, "Ok(Shutdown)");
    assert_clean(&world, "1");
    Ok(())
}

fn creates_and_records_a_missing_set(test: &str) -> TestResult<()> {
    let mut world = World::new("T12b", 1, false)?;
    started(&mut world, test)?;
    world.wait_for_event("the session of a created set", 60, |events| {
        events.iter().any(|event| event.label == "session.create")
    });
    assert_eq!(world.gh("scaleset.create").len(), 1);
    assert_eq!(
        recorded(&world)?["lab"],
        7,
        "the created set must be recorded"
    );
    world.stop()?;
    assert_eq!(world.finish(30)?, "Ok(Shutdown)");
    Ok(())
}

fn refuses_a_set_the_record_assigns_to_another_id(test: &str) -> TestResult<()> {
    let mut world = World::new("T12c", 1, true)?;
    std::fs::write(world.state.join(RECORDS), r#"{"lab":9}"#)?;
    started(&mut world, test)?;
    world.wait_for_event("the lookup of the set", 60, |events| {
        events.iter().any(|event| event.label == "scaleset.get")
    });
    heartbeat_advances(&world, 2);
    assert!(
        world.gh("session.create").is_empty(),
        "a set the record does not own was adopted"
    );
    assert_eq!(recorded(&world)?["lab"], 9);
    world.stop()?;
    assert_eq!(world.finish(30)?, "Ok(Shutdown)");
    Ok(())
}
