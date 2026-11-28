// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde_json::{Value, json};

use super::checks::{assert_clean, heartbeat_advances};
use super::events::wait_for;
use super::fixtures::TestResult;
use super::harness::World;

const RECORD: &str = "rust-runner-scale-sets/lab";

fn recorded(world: &World) -> TestResult<Value> {
    Ok(serde_json::from_str(&std::fs::read_to_string(
        world.state.join(RECORD),
    )?)?)
}

fn plant_record(world: &World, scale_set_id: Option<i64>) -> TestResult<()> {
    plant_scoped(
        world,
        scale_set_id,
        &json!({"kind": "organization", "organization": "tailrocks", "runner_group": null}),
    )
}

fn plant_scoped(world: &World, scale_set_id: Option<i64>, scope: &Value) -> TestResult<()> {
    let record = json!({
        "name": "velnor-mac",
        "runner_group_id": 1,
        "scope": scope,
        "server": "https://pipelines.example.com/",
        "scale_set_id": scale_set_id,
    });
    std::fs::create_dir_all(world.state.join("rust-runner-scale-sets"))?;
    std::fs::write(world.state.join(RECORD), record.to_string())?;
    Ok(())
}

fn started(world: &mut World, test: &str, existing_labels: Option<&[&str]>) -> TestResult<()> {
    world.use_organization()?;
    launched(world, test, existing_labels)
}

fn launched(world: &mut World, test: &str, existing_labels: Option<&[&str]>) -> TestResult<()> {
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
    assert_eq!(world.finish(30)?, "Ok(Shutdown)");
    Ok(())
}

pub(super) fn adopts_a_set_after_a_recorded_intent(test: &str) -> TestResult<()> {
    let mut world = World::new("T12b", 1, true)?;
    plant_record(&world, None)?;
    started(&mut world, test, None)?;
    world.wait_for_event("the session of the set the intent covers", 60, |events| {
        events.iter().any(|event| event.label == "session.create")
    });
    assert!(world.gh("scaleset.create").is_empty());
    assert_eq!(
        recorded(&world)?["scale_set_id"],
        7,
        "the adopted set must be recorded"
    );
    world.stop()?;
    assert_eq!(world.finish(30)?, "Ok(Shutdown)");
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
    assert_eq!(
        recorded(&world)?["scale_set_id"],
        7,
        "the created set must be recorded"
    );
    world.stop()?;
    assert_eq!(world.finish(30)?, "Ok(Shutdown)");
    Ok(())
}

pub(super) fn refuses_a_set_the_record_assigns_to_another_id(test: &str) -> TestResult<()> {
    let mut world = World::new("T12d", 1, true)?;
    plant_record(&world, Some(9))?;
    started(&mut world, test, None)?;
    held(&world, "a set the record assigns to id 9");
    assert_eq!(recorded(&world)?["scale_set_id"], 9);
    world.stop()?;
    assert_eq!(world.finish(30)?, "Ok(Shutdown)");
    Ok(())
}

pub(super) fn refuses_a_set_with_other_labels(test: &str) -> TestResult<()> {
    let mut world = World::new("T12e", 1, true)?;
    plant_record(&world, Some(7))?;
    started(&mut world, test, Some(&["velnor", "another-label"]))?;
    held(&world, "a recorded set that carries other labels");
    world.stop()?;
    assert_eq!(world.finish(30)?, "Ok(Shutdown)");
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
        || !world.state.join(RECORD).exists(),
    );
    world.stop()?;
    assert_eq!(world.finish(30)?, "Ok(Shutdown)");
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
    assert_eq!(world.finish(30)?, "Ok(Shutdown)");
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
        recorded(&world)?["scale_set_id"].is_null(),
        "a created set that persisted other labels was recorded"
    );
    world.stop()?;
    assert_eq!(world.finish(30)?, "Ok(Shutdown)");
    Ok(())
}

pub(super) fn holds_a_recorded_set_the_listing_does_not_show(test: &str) -> TestResult<()> {
    let mut world = World::new("T12h", 1, false)?;
    plant_record(&world, Some(9))?;
    started(&mut world, test, None)?;
    held(&world, "a recorded set that the listing does not show");
    assert_eq!(recorded(&world)?["scale_set_id"], 9);
    assert!(
        world
            .child_stderr()
            .contains("run `velnor-host disconnect`, then connect again")
    );
    world.stop()?;
    assert_eq!(world.finish(30)?, "Ok(Shutdown)");
    Ok(())
}

pub(super) fn holds_a_repository_set_listed_under_another_id_than_its_record(
    test: &str,
) -> TestResult<()> {
    let mut world = World::new("T12i", 1, true)?;
    plant_scoped(
        &world,
        Some(9),
        &json!({"kind": "repository", "repository_id": 42}),
    )?;
    launched(&mut world, test, None)?;
    held(&world, "a set listed under another id than its record");
    assert_eq!(recorded(&world)?["scale_set_id"], 9);
    assert!(
        world
            .child_stderr()
            .contains("run `velnor-host disconnect`, then connect again")
    );
    world.stop()?;
    assert_eq!(world.finish(30)?, "Ok(Shutdown)");
    Ok(())
}
