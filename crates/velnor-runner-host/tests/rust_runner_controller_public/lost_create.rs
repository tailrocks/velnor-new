// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde_json::json;

use super::child::SUPERVISED;
use super::events::wait_for;
use super::fixtures::{TestResult, host_toml, host_toml_of};
use super::harness::World;
use super::world_files::TOKEN_A;

const RECORD: &str = "rust-runner-scale-sets/lab";

pub(super) fn run(test: &str) -> TestResult<()> {
    let mut world = World::new("L1", 1, false)?;
    world.script_refusals(
        &json!({"scaleset.create.lost": {"statuses": [502], "repeat_last": false}}),
    )?;
    let with_lab = host_toml(&world.root, 1)?;
    world.spawn_controller(test, &[(SUPERVISED, "1")])?;
    world.wait_for_event(
        "the session of the set the lost create made",
        60,
        |events| events.iter().any(|event| event.label == "session.create"),
    );

    let created = world.gh("scaleset.create");
    assert_eq!(created.len(), 1, "the set was created twice");
    assert_eq!(created[0].field("status"), "502");
    assert_eq!(created[0].field("intent"), "true");
    wait_for("the set to be bound to its record", 30, || {
        std::fs::read_to_string(world.state.join(RECORD))
            .ok()
            .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok())
            .is_some_and(|record| record["scale_set_id"] == 7)
    });

    world.write_marker("connection-drain/lab", TOKEN_A)?;
    let without = host_toml_of(&world.root, 1, &[])?;
    world.write_host_toml_text(&without)?;
    world.wait_census("the config without lab", &without, &[]);
    assert_eq!(world.detach(test, "lab", &with_lab)?, "Ok(Deleted)");
    assert_eq!(world.gh("scaleset.delete").len(), 1);
    assert!(world.fake_state()?["scale_sets"]["velnor-lab"].is_null());
    assert!(!world.state.join(RECORD).exists());

    world.stop()?;
    assert_eq!(world.finish(20)?, "Ok(Stopped)");
    assert!(!world.seen.join("unexpected").exists());
    Ok(())
}
