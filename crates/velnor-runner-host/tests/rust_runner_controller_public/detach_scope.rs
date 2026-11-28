// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde_json::json;

use super::child::SUPERVISED;
use super::fixtures::{KEYCHAIN_REFERENCE, Spec, TestResult, host_toml_of};
use super::harness::World;
use super::world_files::TOKEN_A;

pub(super) fn run(test: &str) -> TestResult<()> {
    let lab = Spec::lab(KEYCHAIN_REFERENCE);
    let same_set_in_the_organization = Spec {
        id: "lab-org",
        credential_ref: "keychain:com.tailrocks.velnor.host/lab-org",
        ..lab
    }
    .in_organization();
    let mut world = World::new("S4", 1, false)?;
    let single = host_toml_of(&world.root, 1, &[lab])?;
    let none = host_toml_of(&world.root, 1, &[])?;
    let organization = host_toml_of(&world.root, 1, &[same_set_in_the_organization])?;
    world.spawn_controller(test, &[(SUPERVISED, "1")])?;
    world.wait_census("the lone connection serving", &single, &["lab"]);
    let record: serde_json::Value = serde_json::from_slice(&std::fs::read(
        world.state.join("rust-runner-scale-sets/lab"),
    )?)?;
    assert_eq!(record["scale_set_id"], 7);
    assert_eq!(record["runner_group_id"], 1);
    assert_eq!(
        record["scope"],
        json!({"kind": "repository", "repository_id": 42})
    );
    assert!(
        record["server"]
            .as_str()
            .is_some_and(|url| url.starts_with("https://"))
    );
    world.stop()?;
    assert_eq!(world.finish(20)?, "Ok(Stopped)");

    world.write_marker("connection-drain/lab", TOKEN_A)?;
    world.write_host_toml_text(&none)?;
    world.publish_empty_census(&none)?;
    world.script_refusals(&json!({"set.stall": {"statuses": [8000], "repeat_last": false}}))?;
    let outcome = world.detach_within(test, "lab", &single, 4_000)?;
    assert!(
        outcome == "Ok(Uncertain)" || outcome == "Ok(TimedOut)",
        "{outcome}"
    );
    assert!(world.gh("scaleset.delete").is_empty());
    assert_eq!(world.fake_state()?["scale_sets"]["velnor-lab"], 7);

    world.write_host_toml_text(&organization)?;
    world.publish_empty_census(&organization)?;
    assert_eq!(world.detach(test, "lab", &single)?, "Ok(Deleted)");
    assert_eq!(world.gh("scaleset.delete").len(), 1);
    assert!(world.fake_state()?["scale_sets"]["velnor-lab"].is_null());
    Ok(())
}
