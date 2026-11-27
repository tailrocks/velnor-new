// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde_json::json;

use super::fixtures::TestResult;
use super::harness::World;

pub(super) fn run(test: &str) -> TestResult<()> {
    let mut world = World::new("T13", 1, true)?;
    world.script_refusals(&json!({"scaleset.get": {"replies": [{"status": 503}]}}))?;
    world.spawn_controller(test, &[])?;
    world.wait_for_event("the held lookup of the scale set", 60, |events| {
        events
            .iter()
            .any(|event| event.label == "scaleset.get" && event.field("status") == "503")
    });
    world.write_host_toml(2)?;
    assert_eq!(
        world.finish(20)?,
        "Ok(ConfigChanged)",
        "a held startup must notice a changed host.toml before its 30 s retry"
    );
    assert_eq!(world.gh("scaleset.get").len(), 1);
    assert!(world.gh("session.create").is_empty());
    Ok(())
}
