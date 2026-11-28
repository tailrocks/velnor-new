// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde_json::{Value, json};

use super::checks::{assert_clean, runner_name};
use super::events::wait_for;
use super::fixtures::{TestResult, WORKFLOW, push_offer};
use super::harness::World;

const FOREIGN_ID: u64 = 300;

pub(super) fn leaves_a_runner_of_another_scale_set(test: &str) -> TestResult<()> {
    let mut world = World::new("T14", 1, true)?;
    let state_path = world.github.join("state.json");
    let mut state: Value = serde_json::from_str(&std::fs::read_to_string(&state_path)?)?;
    state["runners"] = json!([{
        "id": FOREIGN_ID,
        "name": runner_name(1),
        "runnerScaleSetId": 99
    }]);
    std::fs::write(&state_path, state.to_string())?;
    world.queue_message(141, &[push_offer(1, 1401, "main", WORKFLOW)])?;
    world.spawn_controller(test, &[])?;
    let line = "rust_runner_job=cleaned reservation=1 ";
    wait_for("the release of the slot of the job", 60, || {
        world
            .child_stderr()
            .lines()
            .any(|candidate| candidate.starts_with(line))
    });
    let state: Value = serde_json::from_str(&std::fs::read_to_string(&state_path)?)?;
    let remaining: Vec<u64> = state["runners"]
        .as_array()
        .map_or_else(Vec::new, |runners| {
            runners
                .iter()
                .filter_map(|runner| runner["id"].as_u64())
                .collect()
        });
    assert_eq!(
        remaining,
        [FOREIGN_ID],
        "the runner of another scale set was removed, or ours was left"
    );
    world.stop()?;
    assert_eq!(world.finish(30)?, "Ok(Shutdown)");
    assert_clean(&world, "1");
    Ok(())
}
