// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::child::PASSED_MAX_JOBS;
use super::fixtures::{HOST_TOML_LIMIT, TestResult, host_toml, padded};
use super::harness::World;

pub(super) fn run(test: &str) -> TestResult<()> {
    let mut at_limit = World::new("T18a", 1, true)?;
    let contents = padded(&host_toml(&at_limit.root, 1)?, HOST_TOML_LIMIT);
    assert_eq!(contents.len(), HOST_TOML_LIMIT);
    at_limit.write_host_toml_text(&contents)?;
    at_limit.spawn_controller(test, &[(PASSED_MAX_JOBS, "1")])?;
    at_limit.wait_for_event(
        "a session from a host.toml of exactly the limit",
        60,
        |events| events.iter().any(|event| event.label == "session.create"),
    );
    at_limit.stop()?;
    assert_eq!(at_limit.finish(30)?, "Ok(Shutdown)");

    let mut over = World::new("T18b", 1, true)?;
    let contents = padded(&host_toml(&over.root, 1)?, HOST_TOML_LIMIT + 1);
    assert_eq!(contents.len(), HOST_TOML_LIMIT + 1);
    over.write_host_toml_text(&contents)?;
    over.spawn_controller(test, &[(PASSED_MAX_JOBS, "1")])?;
    assert_eq!(over.finish(30)?, "Err(invalid config)");
    assert!(over.gh("session.create").is_empty());
    Ok(())
}
