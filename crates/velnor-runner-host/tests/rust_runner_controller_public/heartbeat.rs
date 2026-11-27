// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::json;
use velnor_runner_host::{Readiness, controller_readiness};

use super::fixtures::{TestResult, hex_sha256};
use super::harness::World;

const STALE_MS: u64 = 31_000;

fn beat(world: &World, hash: &str, phase: &str, age_ms: u64) -> TestResult<()> {
    let now = u64::try_from(SystemTime::now().duration_since(UNIX_EPOCH)?.as_millis())?;
    std::fs::write(
        world.state.join("rust-runner-controller.json"),
        json!({"config_sha256": hash, "phase": phase, "updated_unix_ms": now - age_ms}).to_string(),
    )?;
    Ok(())
}

pub(super) fn run() -> TestResult<()> {
    let world = World::new("T11", 1, true)?;
    let hash = hex_sha256(&std::fs::read(world.state.join("host.toml"))?);
    for (phase, fresh) in [
        ("ready", Readiness::Ready),
        ("startup", Readiness::Reconciling),
        ("waiting_for_credentials", Readiness::WaitingForCredentials),
    ] {
        beat(&world, &hash, phase, 0)?;
        assert_eq!(
            controller_readiness(&world.state),
            fresh,
            "a fresh {phase} heartbeat"
        );
        beat(&world, &hash, phase, STALE_MS)?;
        assert_eq!(
            controller_readiness(&world.state),
            Readiness::Degraded,
            "a {phase} heartbeat older than 30 s must read as Degraded"
        );
    }
    Ok(())
}
