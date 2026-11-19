// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::HashMap;

use crate::config::RunnerPlatform;
use crate::journal::{StorageProvisioningPlan, StorageRole};
use crate::launch::bootstrap::containers::pair::PairSpecContext;
use crate::worker::{ResourceBudgetConfig, WorkerIdentityNames};

use super::runner_create;

#[test]
fn the_local_runner_turns_the_crates_mirror_off_and_keeps_the_production_environment()
-> Result<(), Box<dyn std::error::Error>> {
    let physical: Vec<_> = StorageRole::PHYSICAL
        .iter()
        .map(|role| (*role, 1 << 20))
        .collect();
    let plan = StorageProvisioningPlan::for_test(&physical);
    let names = WorkerIdentityNames::from_nonce(&"a".repeat(32))?;
    let budget = ResourceBudgetConfig {
        runner_cpu_millicores: 2_000,
        runner_memory_bytes: 4 << 30,
        dind_cpu_millicores: 1_000,
        dind_memory_bytes: 2 << 30,
        operational_memory_reserve_bytes: 1 << 30,
    }
    .validate()?;
    let context = PairSpecContext {
        plan: &plan,
        names: &names,
        connection_id: "lab",
        budget,
    };
    let runner = runner_create(
        &names,
        format!("sha256:{}", "b".repeat(64)),
        &"c".repeat(64),
        RunnerPlatform::LinuxArm64,
        &context,
        HashMap::new(),
    )?;
    let env = runner.config.env.unwrap_or_default();
    assert!(env.iter().any(|item| item == "VELNOR_CRATES_MIRROR=off"));
    assert!(env.iter().any(|item| item.starts_with("DOCKER_HOST=")));
    assert!(
        env.iter()
            .any(|item| item.starts_with("ACTIONS_RUNNER_HOOK_JOB_STARTED="))
    );
    Ok(())
}
