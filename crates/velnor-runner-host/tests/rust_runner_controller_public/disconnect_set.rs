// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde_json::json;

use super::checks::runner_name;
use super::child::SUPERVISED;
use super::events::wait_for;
use super::fixtures::{KEYCHAIN_REFERENCE, Spec, TestResult, host_toml_of};
use super::harness::World;
use super::world_files::{CENSUS, TOKEN_A, now_unix_ms};

const RECORD: &str = "rust-runner-scale-sets/lab";

fn set_listed(world: &World) -> TestResult<bool> {
    Ok(!world.fake_state()?["scale_sets"]["velnor-lab"].is_null())
}

fn deletes(world: &World) -> Vec<String> {
    world
        .gh("scaleset.delete")
        .iter()
        .map(|event| event.field("status").to_owned())
        .collect()
}

fn held_record(
    world: &World,
    test: &str,
    both: &str,
    fields: &[(&str, serde_json::Value)],
) -> TestResult<()> {
    let path = world.state.join(RECORD);
    let original = std::fs::read(&path)?;
    let mut record: serde_json::Value = serde_json::from_slice(&original)?;
    for (field, value) in fields {
        record[*field] = value.clone();
    }
    std::fs::write(&path, record.to_string())?;
    assert_eq!(
        world.detach(test, "lab", both)?,
        "Ok(Uncertain)",
        "{fields:?}"
    );
    assert!(world.gh("scaleset.delete").is_empty());
    assert!(world.gh("runner.delete").is_empty());
    assert!(set_listed(world)?);
    std::fs::write(&path, original)?;
    Ok(())
}

fn refusals(world: &World, test: &str, both: &str) -> TestResult<()> {
    world.script_phase(&json!({"set.delete": {"statuses": [409], "repeat_last": false}}))?;
    assert_eq!(world.detach(test, "lab", both)?, "Ok(Refused(409))");
    assert_eq!(deletes(world), ["409"]);
    assert!(world.gh("runner.delete").iter().any(|event| {
        event.field("name") == runner_name(9999) && event.field("status") == "204"
    }));
    assert!(set_listed(world)?);

    world.script_phase(&json!({"set.delete": {"statuses": [404], "repeat_last": false}}))?;
    assert_eq!(world.detach(test, "lab", both)?, "Ok(Uncertain)");
    assert_eq!(deletes(world), ["409", "404"]);
    let last = world.gh("scaleset.delete")[1].seq;
    assert!(
        world
            .gh("scaleset.get_id")
            .iter()
            .any(|get| get.seq > last && get.field("found") == "true")
    );
    assert!(set_listed(world)?);
    assert!(world.state.join(RECORD).exists());

    world.script_phase(&json!({
        "set.delete": {"statuses": [404], "repeat_last": false},
        "set.get_id": {"statuses": [200, 404], "repeat_last": false}
    }))?;
    assert_eq!(world.detach(test, "lab", both)?, "Ok(Uncertain)");
    let last = world.gh("scaleset.delete")[2].seq;
    assert!(
        world
            .gh("scaleset.get_id")
            .iter()
            .any(|get| get.seq > last && get.field("found") == "false")
    );
    assert!(world.gh("scaleset.get").iter().any(|get| {
        get.seq > last && get.field("repo") == "velnor-lab" && get.field("found") == "true"
    }));
    assert!(set_listed(world)?);

    world.script_phase(&json!({"set.get_id": {"statuses": [404], "repeat_last": false}}))?;
    assert_eq!(world.detach(test, "lab", both)?, "Ok(Uncertain)");
    assert_eq!(deletes(world), ["409", "404", "404"]);
    assert!(set_listed(world)?);

    world.script_phase(&json!({
        "set.delete": {"statuses": [404], "repeat_last": false},
        "set.hide": {"statuses": [1], "repeat_last": false}
    }))?;
    assert_eq!(world.detach(test, "lab", both)?, "Ok(Uncertain)");
    let last = world.gh("scaleset.delete")[3].seq;
    assert!(
        world
            .gh("scaleset.get_id")
            .iter()
            .any(|get| get.seq > last && get.field("found") == "true")
    );
    assert!(set_listed(world)?);

    world.script_phase(&json!({}))?;
    assert_eq!(world.detach(test, "lab", both)?, "Ok(Deleted)");
    assert_eq!(deletes(world), ["409", "404", "404", "404", "204"]);
    assert!(!set_listed(world)?);
    assert!(!world.state.join(RECORD).exists());

    assert_eq!(world.detach(test, "lab", both)?, "Ok(Deleted)");
    assert_eq!(world.gh("scaleset.delete").len(), 5);
    assert_eq!(world.fake_state()?["scale_sets"]["velnor-second"], 8);
    Ok(())
}

pub(super) fn run(test: &str) -> TestResult<()> {
    let (lab, second) = (Spec::lab(KEYCHAIN_REFERENCE), Spec::second());
    let mut world = World::with_extra("X3", 2, false, &[second])?;
    let both = host_toml_of(&world.root, 2, &[lab, second])?;
    let only_second = host_toml_of(&world.root, 2, &[second])?;
    world.write_host_toml_text(&both)?;
    world.write_marker("connection-drain/lab", TOKEN_A)?;
    world.spawn_controller(test, &[(SUPERVISED, "1")])?;
    world.wait_census("both connections serving", &both, &["lab", "second"]);

    world.write_host_toml_text(&only_second)?;
    world.wait_census("the config without lab", &only_second, &["second"]);
    world.plant_runner(&runner_name(9999), 7)?;

    held_record(
        &world,
        test,
        &both,
        &[("server", json!("https://other.example.com/"))],
    )?;
    held_record(&world, test, &both, &[("scale_set_id", json!(999_999))])?;
    held_record(
        &world,
        test,
        &both,
        &[
            ("scale_set_id", json!(null)),
            ("name", json!("velnor-renamed")),
        ],
    )?;
    refusals(&world, test, &both)?;

    world.configure_orb("FAKE_ORB_FAIL=1\n")?;
    let blocked = host_toml_of(&world.root, 3, &[second])?;
    world.write_host_toml_text(&blocked)?;
    world.wait_census(
        "the census of a config whose startup is held",
        &blocked,
        &[],
    );
    world.age(CENSUS, 31_000)?;
    wait_for("a census republished while startup is held", 30, || {
        world
            .census()
            .and_then(|census| census["updated_unix_ms"].as_u64())
            .is_some_and(|stamp| stamp + 10_000 > now_unix_ms())
    });
    assert_eq!(
        world.detach_within(test, "lab", &both, 1_500)?,
        "Ok(TimedOut)"
    );
    assert_ne!(world.heartbeat_phase().as_deref(), Some("ready"));

    world.configure_orb("")?;
    wait_for("a reconciled census after the startup", 120, || {
        world
            .census()
            .is_some_and(|census| census["reconciled"] == true)
    });
    assert_eq!(world.detach(test, "lab", &both)?, "Ok(Deleted)");

    world.stop()?;
    assert_eq!(world.finish(20)?, "Ok(Stopped)");
    assert!(!world.seen.join("unexpected").exists());
    Ok(())
}
