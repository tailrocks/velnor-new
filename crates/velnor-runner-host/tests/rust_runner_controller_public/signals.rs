// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::os::unix::net::UnixListener;

use super::child::HOST_BLOCKING;
use super::events::wait_for;
use super::fixtures::{KEYCHAIN_REFERENCE, TestResult, official_host_toml};
use super::harness::World;

const DEVELOPER_IMAGES: &str = "[developer_images]\nrunner = \"sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\"\ndind = \"sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb\"\nresource_probe = \"sha256:cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc\"\n";

pub(super) fn run(test: &str) -> TestResult<()> {
    let mut world = World::new("T7b", 1, true)?;
    let docker = UnixListener::bind(world.root.join("no-such-docker.sock"))?;
    docker.set_nonblocking(true)?;
    let orb = world.root.join("orb");
    let store = world.store.clone();
    world.spawn_controller(
        test,
        &[
            (HOST_BLOCKING, "1"),
            ("VELNOR_ORB", &orb.to_string_lossy()),
            ("VELNOR_HOST_STORE", &store.to_string_lossy()),
        ],
    )?;
    wait_for("the wait for credentials", 40, || {
        world.heartbeat_phase().as_deref() == Some("waiting_for_credentials")
    });
    world.write_host_toml_text(&format!(
        "{}{DEVELOPER_IMAGES}",
        official_host_toml(&world.root, KEYCHAIN_REFERENCE)
    ))?;
    let mut held = Vec::new();
    wait_for(
        "the official controller to open its Docker connection",
        90,
        || docker.accept().map(|(stream, _)| held.push(stream)).is_ok(),
    );
    world.terminate()?;
    let exit = world.finish(8).map_err(|error| {
        format!("a termination signal did not end the official controller: {error}")
    })?;
    assert_eq!(
        exit, "Ok(())",
        "a termination signal must end the official controller"
    );
    Ok(())
}
