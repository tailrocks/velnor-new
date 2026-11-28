// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde_json::{Value, json};

use super::checks::{assert_clean, heartbeat_advances};
use super::events::wait_for;
use super::fixtures::TestResult;
use super::harness::World;

const RECORDS: &str = "rust-runner-scale-sets.json";

fn recorded(world: &World) -> TestResult<Value> {
    Ok(serde_json::from_str(&std::fs::read_to_string(
        world.state.join(RECORDS),
    )?)?)
}

fn plant_records(world: &World, records: &Value) -> TestResult<()> {
    std::fs::write(world.state.join(RECORDS), records.to_string())?;
    Ok(())
}

fn started(world: &mut World, test: &str, existing_labels: Option<&[&str]>) -> TestResult<()> {
    world.use_organization()?;
    let path = world.github.join("config.json");
    let mut config: Value = serde_json::from_str(&std::fs::read_to_string(&path)?)?;
    config["state"] = json!(world.state);
    if let Some(labels) = existing_labels {
        config["existing_labels"] = json!(labels);
    }
    std::fs::write(path, config.to_string())?;
    world.spawn_controller(test, &[])?;
    Ok(())
}

fn held(world: &World, what: &str) {
    world.wait_for_event("the lookup of the set", 60, |events| {
        events.iter().any(|event| event.label == "scaleset.get")
    });
    heartbeat_advances(world, 2);
    assert!(world.gh("session.create").is_empty(), "{what}: adopted");
    assert!(
        world.gh("scaleset.create").is_empty(),
        "{what}: a set was created"
    );
    assert!(
        world.gh("runner.delete").is_empty(),
        "{what}: a runner of the set was deleted"
    );
}

pub(super) fn holds_a_matching_set_nobody_recorded(test: &str) -> TestResult<()> {
    let mut world = World::new("T12a", 1, true)?;
    world.write_state(true, &[("another-host-1", 300)])?;
    started(&mut world, test, None)?;
    held(&world, "a same-named set made by another host");
    let state: Value =
        serde_json::from_str(&std::fs::read_to_string(world.github.join("state.json"))?)?;
    assert_eq!(
        state["runners"].as_array().map(Vec::len),
        Some(1),
        "the runners of a set nobody recorded were cleared"
    );
    world.stop()?;
    assert_eq!(world.finish()?, "Ok(Shutdown)");
    Ok(())
}

pub(super) fn adopts_a_set_after_a_recorded_intent(test: &str) -> TestResult<()> {
    let mut world = World::new("T12b", 1, true)?;
    plant_records(&world, &json!({"intended": ["lab"]}))?;
    started(&mut world, test, None)?;
    world.wait_for_event("the session of the set the intent covers", 60, |events| {
        events.iter().any(|event| event.label == "session.create")
    });
    assert!(world.gh("scaleset.create").is_empty());
    let records = recorded(&world)?;
    assert_eq!(
        records["created"]["lab"], 7,
        "the adopted set must be recorded"
    );
    assert_eq!(
        records["intended"],
        json!([]),
        "the intent must end with the record"
    );
    world.stop()?;
    assert_eq!(world.finish()?, "Ok(Shutdown)");
    assert_clean(&world, "1");
    Ok(())
}

pub(super) fn records_the_intent_before_it_creates_a_set(test: &str) -> TestResult<()> {
    let mut world = World::new("T12c", 1, false)?;
    started(&mut world, test, None)?;
    world.wait_for_event("the session of a created set", 60, |events| {
        events.iter().any(|event| event.label == "session.create")
    });
    let created = world.gh("scaleset.create");
    assert_eq!(created.len(), 1);
    assert_eq!(
        created[0].field("intent"),
        "true",
        "the create-intent must be on disk before the create request"
    );
    let records = recorded(&world)?;
    assert_eq!(
        records["created"]["lab"], 7,
        "the created set must be recorded"
    );
    assert_eq!(
        records["intended"],
        json!([]),
        "the intent must end with the record"
    );
    world.stop()?;
    assert_eq!(world.finish()?, "Ok(Shutdown)");
    Ok(())
}

pub(super) fn refuses_a_set_the_record_assigns_to_another_id(test: &str) -> TestResult<()> {
    let mut world = World::new("T12d", 1, true)?;
    plant_records(&world, &json!({"created": {"lab": 9}}))?;
    started(&mut world, test, None)?;
    held(&world, "a set the record assigns to id 9");
    assert_eq!(recorded(&world)?["created"]["lab"], 9);
    world.stop()?;
    assert_eq!(world.finish()?, "Ok(Shutdown)");
    Ok(())
}

pub(super) fn refuses_a_set_with_other_labels(test: &str) -> TestResult<()> {
    let mut world = World::new("T12e", 1, true)?;
    plant_records(&world, &json!({"created": {"lab": 7}}))?;
    started(&mut world, test, Some(&["velnor", "another-label"]))?;
    held(&world, "a recorded set that carries other labels");
    world.stop()?;
    assert_eq!(world.finish()?, "Ok(Shutdown)");
    Ok(())
}

pub(super) fn forgets_its_intent_after_a_refused_create(test: &str) -> TestResult<()> {
    let mut world = World::new("T12f", 1, false)?;
    world.script_refusals(&json!({"scaleset.create": {"statuses": [409], "repeat_last": true}}))?;
    started(&mut world, test, None)?;
    world.wait_for_event("the refused create", 60, |events| {
        events
            .iter()
            .any(|event| event.label == "scaleset.create" && event.field("status") == "409")
    });
    wait_for(
        "the create-intent to be forgotten after a definite refusal",
        20,
        || recorded(&world).is_ok_and(|records| records["intended"] == json!([])),
    );
    world.stop()?;
    assert_eq!(world.finish()?, "Ok(Shutdown)");
    std::fs::remove_file(world.seen.join("stop"))?;
    world.write_state(true, &[("another-host-1", 300)])?;
    started(&mut world, test, None)?;
    let refused = world.gh("scaleset.create")[0].seq;
    world.wait_for_event("the lookup after the restart", 60, |events| {
        events
            .iter()
            .any(|event| event.label == "scaleset.get" && event.seq > refused)
    });
    heartbeat_advances(&world, 2);
    assert!(
        world.gh("session.create").is_empty(),
        "a set made by another host was adopted after a refused create"
    );
    assert!(
        world.gh("runner.delete").is_empty(),
        "a runner of a set made by another host was deleted"
    );
    world.stop()?;
    assert_eq!(world.finish()?, "Ok(Shutdown)");
    Ok(())
}

pub(super) fn refuses_a_created_set_that_keeps_other_labels(test: &str) -> TestResult<()> {
    let mut world = World::new("T12g", 1, false)?;
    started(&mut world, test, Some(&["velnor", "another-label"]))?;
    world.wait_for_event("the create of the set", 60, |events| {
        events.iter().any(|event| event.label == "scaleset.create")
    });
    heartbeat_advances(&world, 3);
    assert!(
        world.gh("session.create").is_empty(),
        "a created set that persisted other labels was used"
    );
    assert!(
        recorded(&world)?["created"]["lab"].is_null(),
        "a created set that persisted other labels was recorded"
    );
    world.stop()?;
    assert_eq!(world.finish()?, "Ok(Shutdown)");
    Ok(())
}
