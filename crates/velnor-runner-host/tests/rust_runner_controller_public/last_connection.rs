// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::checks::heartbeat_advances;
use super::child::SUPERVISED;
use super::events::wait_for;
use super::fixtures::{TestResult, hex_sha256, host_toml_of};
use super::harness::World;
use super::world_files::TOKEN_A;

pub(super) fn run(test: &str) -> TestResult<()> {
    let mut world = World::new("L3", 1, true)?;
    let single = std::fs::read_to_string(world.state.join("host.toml"))?;
    let empty = host_toml_of(&world.root, 1, &[])?;
    world.write_marker("connection-drain/lab", TOKEN_A)?;
    world.spawn_controller(test, &[(SUPERVISED, "1")])?;
    world.wait_census("the lone connection serving", &single, &["lab"]);

    world.write_host_toml_text(&empty)?;
    world.wait_census("the census of the config without connections", &empty, &[]);
    let hash = hex_sha256(empty.as_bytes());
    wait_for("the controller ready for the empty config", 60, || {
        world.heartbeat_hash().as_deref() == Some(hash.as_str())
            && world.heartbeat_phase().as_deref() == Some("ready")
    });
    heartbeat_advances(&world, 2);
    assert_eq!(
        world.census().ok_or("census")?["config_sha256"],
        hash.as_str()
    );

    assert_eq!(world.detach(test, "lab", &single)?, "Ok(NotOwned)");
    assert!(world.gh("scaleset.delete").is_empty());
    assert!(world.gh("runner.delete").is_empty());
    assert_eq!(world.fake_state()?["scale_sets"]["velnor-lab"], 7);

    world.stop()?;
    assert_eq!(world.finish(20)?, "Ok(Stopped)");
    Ok(())
}
