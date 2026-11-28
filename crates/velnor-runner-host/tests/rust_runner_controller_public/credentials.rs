// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::child::{HOST_BLOCKING, SECRET_LOOKUPS};
use super::events::wait_for;
use super::fixtures::{TestResult, hex_sha256, host_toml_for, official_host_toml};
use super::harness::World;

fn file_hash(world: &World) -> TestResult<String> {
    Ok(hex_sha256(&std::fs::read(world.state.join("host.toml"))?))
}

pub(super) fn run(test: &str) -> TestResult<()> {
    let mut world = World::new("T7", 1, true)?;
    let reference = format!(
        "keychain:com.tailrocks.velnor.test-{}/lab",
        hex_sha256(world.root.to_string_lossy().as_bytes())
    );
    world.write_host_toml_text(&host_toml_for(&world.root, 1, &reference)?)?;
    let first = file_hash(&world)?;
    world.spawn_controller(test, &[(HOST_BLOCKING, "1")])?;
    wait_for("the wait for credentials", 40, || {
        world.heartbeat_phase().as_deref() == Some("waiting_for_credentials")
            && world.heartbeat_hash().as_deref() == Some(first.as_str())
    });
    assert!(
        world.events().is_empty() && !world.root.join("orb.log").exists(),
        "the controller started before it had a credential"
    );
    let lookups = std::fs::read_to_string(world.seen.join(SECRET_LOOKUPS)).unwrap_or_default();
    assert!(
        lookups
            .lines()
            .any(|line| line == reference.trim_start_matches("keychain:")),
        "the credential lookups did not reach the injected source: {lookups:?}"
    );
    world.write_host_toml_text(&host_toml_for(&world.root, 2, &reference)?)?;
    let second = file_hash(&world)?;
    wait_for("the heartbeat of the new configuration", 75, || {
        world.heartbeat_hash().as_deref() == Some(second.as_str())
    });
    assert_eq!(
        world.heartbeat_phase().as_deref(),
        Some("waiting_for_credentials")
    );
    world.write_host_toml_text(&official_host_toml(&world.root, &reference))?;
    assert_eq!(world.finish()?, "Err(keychain)");
    assert!(
        world
            .child_stderr()
            .contains("handing over to the official-runner controller")
    );
    assert!(world.events().is_empty());
    Ok(())
}
