// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::os::unix::fs::PermissionsExt;
use std::time::{Duration, Instant};

use velnor_runner_host::{
    HostError, RustRunnerFailure, RustRunnerLauncher, run_rust_runner_doctor_probe,
};

use super::child::SUPERVISED;
use super::events::wait_for;
use super::fixtures::TestResult;
use super::harness::World;

const BUDGET: Duration = Duration::from_secs(5);

fn probe(world: &World) -> Result<serde_json::Value, HostError> {
    let launcher = RustRunnerLauncher::new(world.root.join("orb"), world.store.clone());
    run_rust_runner_doctor_probe(&world.state, &launcher, BUDGET)
}

fn refused(world: &World, failure: RustRunnerFailure) {
    assert!(
        matches!(probe(world), Err(HostError::RustRunner(found)) if found == failure),
        "expected {failure}"
    );
}

pub(super) fn run(test: &str) -> TestResult<()> {
    let mut world = World::new("P4", 1, true)?;
    world.spawn_controller(test, &[(SUPERVISED, "1")])?;
    wait_for("the controller ready", 60, || {
        world.heartbeat_phase().as_deref() == Some("ready")
    });
    world.stop()?;
    assert_eq!(world.finish(20)?, "Ok(Stopped)");

    let report = probe(&world)?;
    assert_eq!(report["state"], "completed");
    assert_eq!(report["report"]["heartbeat"], "fresh");

    let listing = world.root.join("list.json");
    let machines = std::fs::read_to_string(&listing)?;
    std::fs::write(&listing, "[]")?;
    refused(&world, RustRunnerFailure::MachineMissing);
    std::fs::write(&listing, machines)?;

    let binary = world.store.join("bin/velnor-runner");
    std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o644))?;
    refused(&world, RustRunnerFailure::BinaryMissing);
    std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o755))?;

    std::fs::remove_file(&binary)?;
    std::fs::create_dir(&binary)?;
    refused(&world, RustRunnerFailure::BinaryUnsafe);
    std::fs::remove_dir(&binary)?;
    std::fs::write(&binary, "")?;
    std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o755))?;

    let jit = world.store.join("jit");
    std::fs::set_permissions(&jit, std::fs::Permissions::from_mode(0o755))?;
    refused(&world, RustRunnerFailure::JitDirectoryAccessible);
    std::fs::set_permissions(&jit, std::fs::Permissions::from_mode(0o700))?;

    let config = std::fs::read_to_string(world.state.join("host.toml"))?;
    world.write_host_toml_text(&format!("{config}\n# another config\n"))?;
    refused(&world, RustRunnerFailure::HeartbeatNotFresh);
    world.write_host_toml_text(&config)?;
    assert!(probe(&world).is_ok());

    world.configure_orb("FAKE_ORB_HANG=list\n")?;
    let started = Instant::now();
    refused(&world, RustRunnerFailure::OrbTimeout);
    assert!(started.elapsed() < Duration::from_secs(15));
    world.configure_orb("")?;
    assert!(probe(&world).is_ok());

    let lister = world.root.join("slow-ls");
    std::fs::write(&lister, "#!/bin/sh\nexec /bin/sleep 30\n")?;
    std::fs::set_permissions(&lister, std::fs::Permissions::from_mode(0o755))?;
    let slow =
        RustRunnerLauncher::new(world.root.join("orb"), world.store.clone()).with_lister(lister);
    let started = Instant::now();
    assert!(matches!(
        run_rust_runner_doctor_probe(&world.state, &slow, BUDGET),
        Err(HostError::RustRunner(RustRunnerFailure::StoreUnsafe))
    ));
    assert!(started.elapsed() < Duration::from_secs(15));

    world.stamp_ahead("rust-runner-controller.json", 3_000)?;
    assert!(
        probe(&world).is_ok(),
        "a heartbeat a few seconds ahead of the clock is fresh"
    );
    world.stamp_ahead("rust-runner-controller.json", 60_000)?;
    refused(&world, RustRunnerFailure::HeartbeatNotFresh);

    world.stamp_ahead("rust-runner-controller.json", 0)?;
    assert!(probe(&world).is_ok());
    world.age("rust-runner-controller.json", 31_000)?;
    refused(&world, RustRunnerFailure::HeartbeatNotFresh);
    Ok(())
}
