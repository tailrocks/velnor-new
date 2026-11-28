// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::child::SUPERVISED;
use super::fixtures::{KEYCHAIN_REFERENCE, Spec, TestResult, host_toml_of};
use super::harness::World;
use super::world_files::TOKEN_A;

fn record_id(world: &World, connection: &str) -> Option<i64> {
    let text = std::fs::read(
        world
            .state
            .join(format!("rust-runner-scale-sets/{connection}")),
    )
    .ok()?;
    serde_json::from_slice::<serde_json::Value>(&text).ok()?["scale_set_id"].as_i64()
}

fn serve_then_stop(world: &mut World, test: &str, toml: &str, id: &str) -> TestResult<()> {
    std::fs::remove_file(world.seen.join("stop")).or_else(|error| {
        if error.kind() == std::io::ErrorKind::NotFound {
            Ok(())
        } else {
            Err(error)
        }
    })?;
    world.spawn_controller(test, &[(SUPERVISED, "1")])?;
    world.wait_census("the connection serving", toml, &[id]);
    world.stop()?;
    assert_eq!(world.finish(20)?, "Ok(Stopped)");
    Ok(())
}

pub(super) fn run(test: &str) -> TestResult<()> {
    let first = Spec::lab(KEYCHAIN_REFERENCE).in_organization();
    let replacement = Spec {
        id: "lab-b",
        credential_ref: "keychain:com.tailrocks.velnor.host/lab-b",
        ..first
    };
    let mut world = World::new("C6", 1, false)?;
    let single = host_toml_of(&world.root, 1, &[first])?;
    let swapped = host_toml_of(&world.root, 1, &[replacement])?;
    let none = host_toml_of(&world.root, 1, &[])?;
    world.write_host_toml_text(&single)?;
    serve_then_stop(&mut world, test, &single, "lab")?;
    assert_eq!(record_id(&world, "lab"), Some(7));

    world.write_marker("connection-drain/lab", TOKEN_A)?;
    world.write_host_toml_text(&swapped)?;
    world.publish_empty_census(&swapped)?;
    assert_eq!(world.detach(test, "lab", &single)?, "Ok(NotOwned)");
    assert!(world.gh("scaleset.delete").is_empty());
    assert_eq!(record_id(&world, "lab"), None);
    assert_eq!(record_id(&world, "lab-b"), Some(7));

    serve_then_stop(&mut world, test, &swapped, "lab-b")?;
    assert_eq!(world.fake_state()?["scale_sets"]["velnor-lab"], 7);

    world.write_marker("connection-drain/lab-b", TOKEN_A)?;
    world.write_host_toml_text(&none)?;
    world.publish_empty_census(&none)?;
    assert_eq!(world.detach(test, "lab-b", &swapped)?, "Ok(Deleted)");
    assert_eq!(world.gh("scaleset.delete").len(), 1);
    assert!(world.fake_state()?["scale_sets"]["velnor-lab"].is_null());
    assert_eq!(record_id(&world, "lab-b"), None);
    Ok(())
}
